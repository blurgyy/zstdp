use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Instant;

use crate::compression::{determine_compression, AcceptedCompression};
use crate::log_request;

/// Longest chunk-size line (size, extensions and CRLF) accepted from upstream.
const MAX_CHUNK_SIZE_LINE: usize = 4096;

/// Reads one chunk-size line, returning its raw bytes (CRLF included) and the size.
fn read_chunk_size_line<R: Read>(reader: &mut R) -> io::Result<(Vec<u8>, usize)> {
    let mut line = Vec::with_capacity(16);
    let mut byte = [0; 1];
    loop {
        reader.read_exact(&mut byte)?;
        line.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
        if line.len() >= MAX_CHUNK_SIZE_LINE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk-size line too long",
            ));
        }
    }

    let text =
        std::str::from_utf8(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // Chunk extensions (`;name=value`) follow the size and carry no meaning here.
    let size_str = text.split(';').next().unwrap_or_default().trim();
    let size = usize::from_str_radix(size_str, 16)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok((line, size))
}

/// Reads the trailer section that ends a chunked body, up to and including the
/// terminating empty line, returning its raw bytes.
fn read_trailer_section<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut section = Vec::new();
    let mut byte = [0; 1];
    loop {
        reader.read_exact(&mut byte)?;
        section.push(byte[0]);
        if section == b"\r\n" || section.ends_with(b"\r\n\r\n") {
            return Ok(section);
        }
        if section.len() >= MAX_CHUNK_SIZE_LINE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunked trailer section too long",
            ));
        }
    }
}

/// Copies a chunked body from `reader` to `writer` verbatim, framing included.
pub fn forward_chunked_body<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> io::Result<(usize, usize)> {
    // Return (bytes_read, bytes_written)
    let start_time = Instant::now();
    let mut total_bytes = 0;

    loop {
        let (size_line, size) = read_chunk_size_line(reader)?;
        total_bytes += size_line.len();
        writer.write_all(&size_line)?;

        if size == 0 {
            log::debug!("Reached end of chunked body, total bytes forwarded: {total_bytes}");
            break;
        }

        total_bytes += size;
        log::debug!("Forwarding chunk of size: {size} bytes");

        io::copy(&mut reader.take(size as u64), writer)?;

        // Read and write the CRLF after the chunk
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        writer.write_all(&crlf)?;
        total_bytes += 2;
    }

    let trailer = read_trailer_section(reader)?;
    writer.write_all(&trailer)?;
    total_bytes += trailer.len();

    log::debug!(
        "Completed chunked body transfer in {:?}",
        start_time.elapsed()
    );

    Ok((total_bytes, total_bytes))
}

/// Reads a chunked body from `reader` and writes only its payload (framing and
/// trailers stripped) to `writer`. Returns the number of bytes read off the wire.
pub fn read_chunked_body<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<usize> {
    let mut wire_bytes = 0;

    loop {
        let (size_line, size) = read_chunk_size_line(reader)?;
        wire_bytes += size_line.len();

        if size == 0 {
            break;
        }

        let copied = io::copy(&mut reader.take(size as u64), writer)?;
        if copied != size as u64 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "chunked body ended mid-chunk",
            ));
        }
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        wire_bytes += size + 2;
    }

    wire_bytes += read_trailer_section(reader)?.len();
    log::debug!("Decoded chunked body of {wire_bytes} bytes on the wire");

    Ok(wire_bytes)
}

/// Reads one response head (status line and header fields, through the empty line).
pub fn read_response_head<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while let Ok(1) = reader.read(&mut byte) {
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    Ok(head)
}

/// What the proxy needs to know about a request it forwarded upstream.
pub struct ForwardedRequest {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub accepted_compression: AcceptedCompression,
}

pub fn forward_request(
    client: &mut TcpStream,
    server: &mut TcpStream,
) -> io::Result<ForwardedRequest> {
    let start_time = Instant::now();
    let mut request = Vec::new();
    let mut headers = Vec::new();
    // All Accept-Encoding field lines, combined into one list (RFC 9110 5.3).
    let mut accept_encoding: Vec<String> = Vec::new();
    let mut method = String::new();
    let mut uri = String::new();
    let mut buf_reader = BufReader::new(client);

    // Read and forward request line
    let mut first_line = String::new();
    buf_reader.read_line(&mut first_line)?;

    // Extract method and URI from request line
    let mut request_line = first_line.split_whitespace();
    if let Some(method_part) = request_line.next() {
        method = method_part.to_string();
    }
    if let Some(uri_part) = request_line.next() {
        uri = uri_part.to_string();
    }

    request.extend_from_slice(first_line.as_bytes());

    // Log the request after we've read it
    log_request!(&first_line);

    // Read headers
    let mut line = String::new();
    while {
        line.clear();
        buf_reader.read_line(&mut line)?;
        !line.trim().is_empty()
    } {
        request.extend_from_slice(line.as_bytes());

        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("accept-encoding") {
                accept_encoding.push(value.trim().to_string());
            }
        }

        if !line.to_lowercase().starts_with("host:") {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                headers.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
            }
        }
    }

    // Forward complete request
    server.write_all(&request)?;
    server.write_all(b"\r\n")?;
    server.flush()?;

    // Forward request body if present
    if let Some(length) = headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "content-length")
        .and_then(|(_, v)| v.parse::<u64>().ok())
    {
        log::debug!("Forwarding request body of {length} bytes");
        io::copy(&mut buf_reader.take(length), server)?;
    }

    log::debug!("Completed request forwarding in {:?}", start_time.elapsed());

    Ok(ForwardedRequest {
        method,
        uri,
        headers,
        accepted_compression: determine_compression(&accept_encoding.join(", ")),
    })
}

pub fn tunnel_connection(client: TcpStream, server: TcpStream) -> io::Result<()> {
    use std::net::Shutdown;

    let client_addr = client.peer_addr()?;
    log::debug!("Creating bi-directional tunnel for connection from {client_addr}");

    // Ensure blocking I/O; ignore errors if already blocking
    let _ = client.set_nonblocking(false);
    let _ = server.set_nonblocking(false);

    // Clone ends for independent directions
    let mut client_reader = client.try_clone()?;
    let mut server_writer = server.try_clone()?;

    // Client -> Server
    let up = thread::spawn(move || -> io::Result<()> {
        let res = io::copy(&mut client_reader, &mut server_writer);
        // Half-close server write when client input ends
        let _ = server_writer.shutdown(Shutdown::Write);
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                match e.kind() {
                    io::ErrorKind::ConnectionReset
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::UnexpectedEof => {
                        log::debug!("Client->Server stream ended: {e}");
                    }
                    _ => log::error!("Error forwarding client to server: {e}"),
                }
                Err(e)
            }
        }
    });

    // Server -> Client (run on current thread)
    let res_down = {
        let mut server_reader = server; // original server handle for reading
        let mut client_writer = client; // original client handle for writing
        let res = io::copy(&mut server_reader, &mut client_writer);
        // Half-close client write when server input ends
        let _ = client_writer.shutdown(Shutdown::Write);
        res
    };

    // Wait for client->server to finish
    let _ = up.join();

    match res_down {
        Ok(_) => {
            log::debug!("Tunnel closed for connection from {client_addr}");
            Ok(())
        }
        Err(e) => match e.kind() {
            io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof => {
                log::debug!("Server->Client stream ended: {e}");
                Ok(())
            }
            _ => {
                log::error!("Error forwarding server to client: {e}");
                Err(e)
            }
        },
    }
}
