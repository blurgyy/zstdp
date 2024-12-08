use clap::{ArgGroup, Parser};
use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
use mime_guess::from_path;
use percent_encoding::percent_decode_str;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use zstd::stream::write::Encoder as ZstdEncoder;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
#[clap(group(ArgGroup::new("mode").required(true).args(&["forward_addr", "serve_dir"])))]
struct Args {
    #[arg(short, long)]
    listen_addr: String,

    #[arg(short, long)]
    forward_addr: Option<String>,

    #[arg(short, long)]
    serve_dir: Option<PathBuf>,

    #[arg(short, long)]
    custom_header: Option<String>,

    #[arg(short, long, default_value = "3")]
    zstd_level: i32,

    #[arg(short, long, default_value = "6")]
    gzip_level: u32,
}

#[derive(Debug, PartialEq, Copy, Clone)]
enum CompressionType {
    Zstd,
    Gzip,
    None,
}

struct PrecompressedFile {
    path: PathBuf,
    compression: CompressionType,
}

fn determine_compression(accept_encoding: &str) -> CompressionType {
    let binding = accept_encoding.to_lowercase();
    let encodings: Vec<&str> = binding.split(',').map(|s| s.trim()).collect();
    if encodings.iter().any(|&e| e == "zstd") {
        CompressionType::Zstd
    } else if encodings.iter().any(|&e| e == "gzip") {
        CompressionType::Gzip
    } else {
        CompressionType::None
    }
}

fn find_precompressed(
    base_dir: &Path,
    path: &Path,
    requested_compression: CompressionType,
) -> io::Result<Option<PrecompressedFile>> {
    // Only look for precompressed files if compression is requested
    if requested_compression == CompressionType::None {
        return Ok(None);
    }

    // Check for precompressed files in order of preference
    let possible_paths = match requested_compression {
        CompressionType::Zstd => vec![(path.with_extension("zst"), CompressionType::Zstd)],
        CompressionType::Gzip => vec![(path.with_extension("gz"), CompressionType::Gzip)],
        CompressionType::None => vec![],
    };

    // Try each possible precompressed file
    for (compressed_path, compression) in possible_paths {
        if let Some(validated_path) = sanitize_path(base_dir, &compressed_path.to_string_lossy())? {
            if validated_path.exists() {
                return Ok(Some(PrecompressedFile {
                    path: validated_path,
                    compression,
                }));
            }
        }
    }

    Ok(None)
}

fn sanitize_path(base_dir: &Path, request_path: &str) -> io::Result<Option<PathBuf>> {
    let canonical_base = fs::canonicalize(base_dir)?;

    let decoded_path = percent_decode_str(request_path)
        .decode_utf8()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let requested_path = canonical_base.join(
        PathBuf::from(decoded_path.as_ref())
            .components()
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .collect::<PathBuf>(),
    );

    let canonical_requested = match fs::canonicalize(&requested_path) {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => requested_path,
        Err(e) => return Err(e),
    };

    if canonical_requested.starts_with(&canonical_base) {
        Ok(Some(canonical_requested))
    } else {
        Ok(None)
    }
}

struct FileResponse {
    content: Vec<u8>,
    mime_type: String,
    compression: CompressionType,
}

fn serve_file(
    base_dir: &Path,
    request_path: &str,
    compression: CompressionType,
    zstd_level: i32,
    gzip_level: u32,
) -> io::Result<Option<FileResponse>> {
    let path = match sanitize_path(base_dir, request_path)? {
        Some(p) => p,
        None => return Ok(None),
    };

    let final_path = if path.is_dir() {
        path.join("index.html")
    } else {
        path
    };

    let final_path = match sanitize_path(base_dir, &final_path.to_string_lossy())? {
        Some(p) => p,
        None => return Ok(None),
    };

    let (actual_path, pre_compressed_type) =
        if let Some(precompressed) = find_precompressed(base_dir, &final_path, compression)? {
            (precompressed.path, Some(precompressed.compression))
        } else {
            (final_path.clone(), None)
        };

    if !actual_path.exists() {
        return Ok(None);
    }

    let metadata = fs::metadata(&actual_path)?;
    if !metadata.is_file() {
        return Ok(None);
    }

    let mut content = Vec::new();
    File::open(&actual_path)?.read_to_end(&mut content)?;

    let mime_type = from_path(&final_path).first_or_octet_stream().to_string();

    let (final_content, final_compression) = match pre_compressed_type {
        Some(comp_type) => (content, comp_type),
        None => match compression {
            CompressionType::Zstd => {
                let mut encoder = ZstdEncoder::new(Vec::new(), zstd_level)?;
                encoder.write_all(&content)?;
                (encoder.finish()?, CompressionType::Zstd)
            }
            CompressionType::Gzip => {
                let mut encoder = GzEncoder::new(Vec::new(), GzipCompression::new(gzip_level));
                encoder.write_all(&content)?;
                (encoder.finish()?, CompressionType::Gzip)
            }
            CompressionType::None => (content, CompressionType::None),
        },
    };

    Ok(Some(FileResponse {
        content: final_content,
        mime_type,
        compression: final_compression,
    }))
}

fn handle_connection(client: TcpStream, args: &Args) -> io::Result<()> {
    match (&args.forward_addr, &args.serve_dir) {
        (Some(forward_addr), None) => handle_proxy_connection(
            client,
            forward_addr,
            args.custom_header.clone(),
            args.zstd_level,
        ),
        (None, Some(serve_dir)) => {
            let mut buf_reader = BufReader::new(&client);
            let mut first_line = String::new();
            buf_reader.read_line(&mut first_line)?;

            let mut headers = Vec::new();
            let mut line = String::new();
            while {
                line.clear();
                buf_reader.read_line(&mut line)?;
                !line.trim().is_empty()
            } {
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    headers.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
                }
            }

            handle_file_request(
                client,
                serve_dir,
                &first_line,
                &headers,
                args.zstd_level,
                args.gzip_level,
            )
        }
        _ => unreachable!("Clap group ensures exactly one mode is selected"),
    }
}

fn handle_file_request(
    mut client: TcpStream,
    base_dir: &Path,
    request: &str,
    headers: &[(String, String)],
    zstd_level: i32,
    gzip_level: u32,
) -> io::Result<()> {
    let accept_encoding = headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "accept-encoding")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    let compression = determine_compression(accept_encoding);

    let request_path = request.split_whitespace().nth(1).unwrap_or("/");

    match serve_file(base_dir, request_path, compression, zstd_level, gzip_level)? {
        Some(response) => {
            client.write_all(b"HTTP/1.1 200 OK\r\n")?;
            client.write_all(format!("Content-Type: {}\r\n", response.mime_type).as_bytes())?;

            match response.compression {
                CompressionType::Zstd => {
                    client.write_all(b"Content-Encoding: zstd\r\n")?;
                }
                CompressionType::Gzip => {
                    client.write_all(b"Content-Encoding: gzip\r\n")?;
                }
                CompressionType::None => {}
            }

            client
                .write_all(format!("Content-Length: {}\r\n", response.content.len()).as_bytes())?;
            client.write_all(b"\r\n")?;
            client.write_all(&response.content)?;
        }
        None => {
            client.write_all(b"HTTP/1.1 404 Not Found\r\n")?;
            client.write_all(b"Content-Type: text/plain\r\n")?;
            client.write_all(b"Content-Length: 9\r\n")?;
            client.write_all(b"\r\n")?;
            client.write_all(b"Not Found")?;
        }
    }

    Ok(())
}

fn forward_request(
    client: &mut TcpStream,
    server: &mut TcpStream,
    custom_header: &Option<String>,
) -> io::Result<(Vec<(String, String)>, bool)> {
    let mut request = Vec::new();
    let mut headers = Vec::new();
    let mut supports_zstd = false;
    let mut buf_reader = BufReader::new(client);

    // Read request line and headers
    let mut line = String::new();
    while {
        line.clear();
        buf_reader.read_line(&mut line)?;
        !line.trim().is_empty()
    } {
        request.extend_from_slice(line.as_bytes());
        if line.to_lowercase().starts_with("accept-encoding:")
            && line.to_lowercase().contains("zstd")
        {
            supports_zstd = true;
        }
        if !line.to_lowercase().starts_with("host:") {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                headers.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
            }
        }
    }

    // Add custom header if provided
    if let Some(header) = custom_header {
        request.extend_from_slice(format!("{}\r\n", header).as_bytes());
    }

    // Forward request to server
    server.write_all(&request)?;
    server.write_all(b"\r\n")?;

    // Forward request body if present
    if let Some(length) = headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "content-length")
        .and_then(|(_, v)| v.parse::<u64>().ok())
    {
        io::copy(&mut buf_reader.take(length), server)?;
    }

    Ok((headers, supports_zstd))
}

fn parse_response_headers(headers: &str) -> (&str, Vec<(String, String)>) {
    let mut lines = headers.lines();
    let status_line = lines.next().unwrap_or("");
    let headers = lines
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
            } else {
                None
            }
        })
        .collect();
    (status_line, headers)
}

fn forward_chunked_body<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<()> {
    loop {
        let mut size_buf = [0; 16];
        let mut size_bytes = 0;
        // Read chunk size
        loop {
            let byte = &mut [0; 1];
            reader.read_exact(byte)?;
            size_buf[size_bytes] = byte[0];
            size_bytes += 1;
            if byte[0] == b'\n' {
                break;
            }
        }
        writer.write_all(&size_buf[..size_bytes])?;
        let size_str = std::str::from_utf8(&size_buf[..size_bytes - 2])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if size == 0 {
            break;
        }
        io::copy(&mut reader.take(size as u64), writer)?;
        // Read and write the CRLF after the chunk
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        writer.write_all(&crlf)?;
    }
    // Forward the final CRLF after the last chunk
    let mut final_crlf = [0; 2];
    reader.read_exact(&mut final_crlf)?;
    writer.write_all(&final_crlf)?;
    Ok(())
}

fn handle_proxy_connection(
    mut client: TcpStream,
    forward_addr: &str,
    custom_header: Option<String>,
    zstd_level: i32,
) -> io::Result<()> {
    let mut server = TcpStream::connect(forward_addr)?;

    // Forward request to server
    let (_, supports_zstd) = forward_request(&mut client, &mut server, &custom_header)?;

    // Read and forward response
    let mut response_headers = Vec::new();
    let mut byte = [0u8; 1];
    while let Ok(1) = server.read(&mut byte) {
        response_headers.push(byte[0]);
        if response_headers.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let response_headers_str = String::from_utf8_lossy(&response_headers).to_string();
    let (status_line, headers) = parse_response_headers(&response_headers_str);

    let chunked = headers.iter().any(|(k, v)| {
        k.to_lowercase() == "transfer-encoding" && v.to_lowercase().contains("chunked")
    });

    let content_length = headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok());

    // Modify headers for zstd if client supports it
    let mut modified_headers = headers.clone();
    if supports_zstd {
        modified_headers.retain(|(k, _)| k.to_lowercase() != "content-length");
        modified_headers.push(("Content-Encoding".to_string(), "zstd".to_string()));
        modified_headers.push(("Transfer-Encoding".to_string(), "chunked".to_string()));
    }

    // Send modified headers to client
    client.write_all(format!("{}\r\n", status_line).as_bytes())?;
    for (key, value) in &modified_headers {
        client.write_all(format!("{}: {}\r\n", key, value).as_bytes())?;
    }
    client.write_all(b"\r\n")?;

    // Forward body with optional compression
    if supports_zstd {
        let mut encoder = ZstdEncoder::new(Vec::new(), zstd_level)?;
        if chunked {
            forward_chunked_body(&mut server, &mut encoder)?;
        } else if let Some(length) = content_length {
            io::copy(&mut server.take(length as u64), &mut encoder)?;
        } else {
            io::copy(&mut server, &mut encoder)?;
        }
        let compressed = encoder.finish()?;
        let mut chunked_writer = BufWriter::new(&mut client);
        for chunk in compressed.chunks(8192) {
            write!(chunked_writer, "{:X}\r\n", chunk.len())?;
            chunked_writer.write_all(chunk)?;
            write!(chunked_writer, "\r\n")?;
        }
        write!(chunked_writer, "0\r\n\r\n")?;
        chunked_writer.flush()?;
    } else {
        if chunked {
            forward_chunked_body(&mut server, &mut client)?;
        } else if let Some(length) = content_length {
            io::copy(&mut server.take(length as u64), &mut client)?;
        } else {
            io::copy(&mut server, &mut client)?;
        }
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let listener = TcpListener::bind(&args.listen_addr)?;
    println!("Listening on: {}", args.listen_addr);

    match (&args.forward_addr, &args.serve_dir) {
        (Some(addr), None) => println!("Forwarding to: {}", addr),
        (None, Some(dir)) => println!("Serving directory: {}", dir.display()),
        _ => unreachable!(),
    }

    for stream in listener.incoming() {
        let stream = stream?;
        let args = args.clone();

        thread::spawn(move || {
            if let Err(e) = handle_connection(stream, &args) {
                eprintln!("Error handling connection: {}", e);
            }
        });
    }

    Ok(())
}
