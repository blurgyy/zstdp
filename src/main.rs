use clap::Parser;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use zstd::stream::write::Encoder as ZstdEncoder;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    listen_addr: String,

    #[arg(short, long)]
    forward_addr: String,

    #[arg(short, long)]
    custom_header: Option<String>,

    #[arg(short, long, default_value = "3")]
    zstd_level: i32,
}

fn handle_connection(
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
    client.write_all(format!("{}\n", status_line).as_bytes())?;
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
            // 8KB chunks, adjust as needed
            write!(chunked_writer, "{:X}\r\n", chunk.len())?;
            chunked_writer.write_all(chunk)?;
            write!(chunked_writer, "\r\n")?;
        }
        write!(chunked_writer, "0\r\n\r\n")?; // End of chunked data
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

fn main() -> io::Result<()> {
    let args = Args::parse();
    let listener = TcpListener::bind(&args.listen_addr)?;
    println!("Listening on: {}", args.listen_addr);
    println!("Forwarding to: {}", args.forward_addr);

    for stream in listener.incoming() {
        let stream = stream?;
        let forward_addr = args.forward_addr.clone();
        let custom_header = args.custom_header.clone();
        thread::spawn(move || {
            if let Err(e) = handle_connection(stream, &forward_addr, custom_header, args.zstd_level)
            {
                eprintln!("Error handling connection: {}", e);
            }
        });
    }
    Ok(())
}
