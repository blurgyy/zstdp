use io::BufWriter;

use crate::logging::LoggingExt;

use super::headers::parse_response_headers;
use super::transfer::{forward_chunked_body, forward_request};
use super::*;
use std::time::Instant;

pub fn handle_proxy_connection(
    mut client: TcpStream,
    forward: &str,
    zstd_level: i32,
) -> io::Result<()> {
    let start_time = Instant::now();
    log::debug!("→ New proxy connection to {}", forward);

    let mut server = TcpStream::connect(forward).map_err(|e| {
        log::error!("Failed to connect to backend {}: {}", forward, e);
        e
    })?;
    log::debug!("Connected to backend server in {:?}", start_time.elapsed());

    // Forward request to server
    let (_headers, supports_zstd) = forward.log_operation("forward_request", || {
        forward_request(&mut client, &mut server.try_clone()?)
    })?;

    // Read response headers
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
    log::debug!("← {} from backend", status_line);

    // Check compression and encoding properties
    let current_encoding = headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "content-encoding")
        .map(|(_, v)| v.to_lowercase());

    let is_already_compressed = current_encoding.is_some();
    let is_chunked = headers.iter().any(|(k, v)| {
        k.to_lowercase() == "transfer-encoding" && v.to_lowercase().contains("chunked")
    });

    let content_length = headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok());

    log::debug!(
        "Response properties - compressed: {}, chunked: {}, length: {:?}",
        is_already_compressed,
        is_chunked,
        content_length
    );

    if is_already_compressed {
        forward.log_operation("forward_compressed", || {
            // Forward headers and body as-is
            client.write_all(&response_headers)?;

            if is_chunked {
                forward_chunked_body(&mut server.try_clone()?, &mut client)
            } else if let Some(length) = content_length {
                io::copy(&mut server.take(length as u64), &mut client)?;
                Ok(())
            } else {
                io::copy(&mut server, &mut client)?;
                Ok(())
            }
        })?;
    } else {
        forward.log_operation("forward_with_compression", || {
            let mut modified_headers = headers.clone();
            if supports_zstd {
                modified_headers.retain(|(k, _)| k.to_lowercase() != "content-length");
                modified_headers.push(("Content-Encoding".to_string(), "zstd".to_string()));
                modified_headers.push(("Transfer-Encoding".to_string(), "chunked".to_string()));
            }

            // Send modified headers
            client.write_all(format!("{}\r\n", status_line).as_bytes())?;
            for (key, value) in &modified_headers {
                client.write_all(format!("{}: {}\r\n", key, value).as_bytes())?;
            }
            client.write_all(b"\r\n")?;

            if supports_zstd {
                let mut encoder = ZstdEncoder::new(Vec::new(), zstd_level)?;
                if is_chunked {
                    forward_chunked_body(&mut server.try_clone()?, &mut encoder)?;
                } else if let Some(length) = content_length {
                    io::copy(&mut server.take(length as u64), &mut encoder)?;
                } else {
                    io::copy(&mut server, &mut encoder)?;
                }

                let compressed = encoder.finish()?;
                log::debug!("Compressed response to {} bytes", compressed.len());

                let mut chunked_writer = BufWriter::new(&mut client);
                for chunk in compressed.chunks(8192) {
                    write!(chunked_writer, "{:X}\r\n", chunk.len())?;
                    chunked_writer.write_all(chunk)?;
                    write!(chunked_writer, "\r\n")?;
                }
                write!(chunked_writer, "0\r\n\r\n")?;
                chunked_writer.flush()
            } else {
                if is_chunked {
                    forward_chunked_body(&mut server.try_clone()?, &mut client)
                } else if let Some(length) = content_length {
                    io::copy(&mut server.take(length as u64), &mut client)?;
                    Ok(())
                } else {
                    io::copy(&mut server, &mut client)?;
                    Ok(())
                }
            }
        })?;
    }

    log::debug!("← Completed proxy request in {:?}", start_time.elapsed());

    Ok(())
}
