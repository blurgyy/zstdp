use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Instant;

use crate::compression::determine_compression;
use crate::log_request;

pub fn forward_chunked_body<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> io::Result<(usize, usize)> {
    // Return (bytes_read, bytes_written)
    let start_time = Instant::now();
    let mut total_bytes_read = 0;
    let mut total_bytes_written = 0;

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

        total_bytes_read += size_bytes;
        total_bytes_written += size_bytes;
        writer.write_all(&size_buf[..size_bytes])?;

        let size_str = std::str::from_utf8(&size_buf[..size_bytes - 2])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        if size == 0 {
            log::debug!(
                "Reached end of chunked body, total bytes read: {}, written: {}",
                total_bytes_read,
                total_bytes_written
            );
            break;
        }

        total_bytes_read += size;
        total_bytes_written += size;
        log::debug!("Forwarding chunk of size: {} bytes", size);

        io::copy(&mut reader.take(size as u64), writer)?;

        // Read and write the CRLF after the chunk
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        writer.write_all(&crlf)?;
        total_bytes_read += 2;
        total_bytes_written += 2;
    }

    // Forward the final CRLF after the last chunk
    let mut final_crlf = [0; 2];
    reader.read_exact(&mut final_crlf)?;
    writer.write_all(&final_crlf)?;
    total_bytes_read += 2;
    total_bytes_written += 2;

    log::debug!(
        "Completed chunked body transfer in {:?}",
        start_time.elapsed()
    );

    Ok((total_bytes_read, total_bytes_written))
}

pub fn forward_request(
    client: &mut TcpStream,
    server: &mut TcpStream,
) -> io::Result<(Vec<(String, String)>, bool, String)> {
    // Add String to return type for URI
    let start_time = Instant::now();
    let mut request = Vec::new();
    let mut headers = Vec::new();
    let mut supports_zstd = false;
    let mut uri = String::new();
    let mut buf_reader = BufReader::new(client);

    // Read and forward request line
    let mut first_line = String::new();
    buf_reader.read_line(&mut first_line)?;

    // Extract URI from request line
    if let Some(uri_part) = first_line.split_whitespace().nth(1) {
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

        if line.to_lowercase().starts_with("accept-encoding:") {
            let accept_encoding = line.split(':').map(|s| s.trim()).collect::<Vec<_>>()[1];
            supports_zstd = determine_compression(accept_encoding).supports_zstd;
            log::debug!("Client accepts zstd compression: {}", supports_zstd);
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
        log::debug!("Forwarding request body of {} bytes", length);
        io::copy(&mut buf_reader.take(length), server)?;
    }

    log::debug!("Completed request forwarding in {:?}", start_time.elapsed());

    Ok((headers, supports_zstd, uri))
}

pub fn tunnel_connection(mut client: TcpStream, mut server: TcpStream) -> io::Result<()> {
    let client_addr = client.peer_addr()?;
    log::debug!(
        "Creating bi-directional tunnel for connection from {}",
        client_addr
    );

    // Enable non-blocking mode for better performance
    client.set_nonblocking(true)?;
    server.set_nonblocking(true)?;

    let mut client_to_server = client.try_clone()?;
    let mut server_to_client = server.try_clone()?;

    // Spawn a thread to handle client -> server
    let client_handle = thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            match client_to_server.read(&mut buffer) {
                Ok(0) => break, // Connection closed
                Ok(n) => {
                    if let Err(err) = server.write_all(&buffer[..n]) {
                        if err.kind() != io::ErrorKind::WouldBlock {
                            log::error!("Error forwarding client to server: {}", err);
                            break;
                        }
                    }
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    // No data available, yield to other thread
                    thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                Err(err) => {
                    log::error!("Error reading from client: {}", err);
                    break;
                }
            }
        }
    });

    // Handle server -> client in the main thread
    let mut buffer = [0; 8192];
    loop {
        match server_to_client.read(&mut buffer) {
            Ok(0) => break, // Connection closed
            Ok(n) => {
                if let Err(err) = client.write_all(&buffer[..n]) {
                    if err.kind() != io::ErrorKind::WouldBlock {
                        log::error!("Error forwarding server to client: {}", err);
                        break;
                    }
                }
            }
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                // No data available, yield to other thread
                thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            Err(err) => {
                log::error!("Error reading from server: {}", err);
                break;
            }
        }
    }

    // Wait for the other direction to complete
    let _ = client_handle.join();

    log::debug!("Tunnel closed for connection from {}", client_addr);
    Ok(())
}
