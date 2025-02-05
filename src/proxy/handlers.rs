use io::BufWriter;
use regex::Regex;
use transfer::tunnel_connection;

use crate::args::should_bypass_compression;
use crate::logging::LoggingExt;

use super::headers::parse_response_headers;
use super::transfer::{forward_chunked_body, forward_request};
use super::*;
use std::sync::Arc;
use std::time::Instant;

pub fn handle_proxy_connection(
    mut client: TcpStream,
    forward: &str,
    zstd_level: i32,
    bypass_patterns: Arc<Vec<Regex>>,
) -> io::Result<(io::Result<()>, usize, usize)> {
    let start_time = Instant::now();
    log::debug!("→ New proxy connection to {}", forward);

    let mut server = TcpStream::connect(forward).map_err(|e| {
        log::error!("Failed to connect to backend {}: {}", forward, e);
        e
    })?;
    log::debug!("Connected to backend server in {:?}", start_time.elapsed());

    let (headers, supports_zstd, uri) = forward.log_operation("forward_request", || {
        forward_request(&mut client, &mut server.try_clone()?)
    })?;

    // Check for WebSocket upgrade request
    let is_websocket = headers
        .iter()
        .any(|(k, v)| k.to_lowercase() == "upgrade" && v.to_lowercase().contains("websocket"));

    if is_websocket {
        log::debug!("WebSocket upgrade request detected, creating tunnel");
        // For WebSocket connections, create a direct tunnel
        return Ok((tunnel_connection(client, server), 0, 0));
    }

    let should_bypass = should_bypass_compression(&uri, &bypass_patterns);
    if should_bypass {
        log::debug!("URI '{}' matches bypass pattern, skipping compression", uri);
    }

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

    let mut original_size = 0;
    let mut final_size = 0;

    let result = if is_already_compressed || should_bypass {
        forward.log_operation("forward_compressed", || {
            client.write_all(&response_headers)?;

            if is_chunked {
                let (bytes_read, bytes_written) =
                    forward_chunked_body(&mut server.try_clone()?, &mut client)?;
                original_size = bytes_read;
                final_size = bytes_written;
                Ok(())
            } else if let Some(length) = content_length {
                original_size = length;
                final_size = length;
                io::copy(&mut server.take(length as u64), &mut client)?;
                Ok(())
            } else {
                let bytes = io::copy(&mut server, &mut client)?;
                original_size = bytes as usize;
                final_size = bytes as usize;
                Ok(())
            }
        })
    } else {
        forward.log_operation("forward_with_compression", || {
            let mut buffer = Vec::new();

            // Read the entire response body
            if is_chunked {
                let (bytes_read, _) = forward_chunked_body(&mut server.try_clone()?, &mut buffer)?;
                original_size = bytes_read;
            } else if let Some(length) = content_length {
                io::copy(&mut server.take(length as u64), &mut buffer)?;
                original_size = length;
            } else {
                let bytes = io::copy(&mut server, &mut buffer)?;
                original_size = bytes as usize;
            }

            let mut modified_headers = headers.clone();
            modified_headers.retain(|(k, _)| k.to_lowercase() != "content-length");

            if supports_zstd {
                modified_headers.push(("Content-Encoding".to_string(), "zstd".to_string()));
                modified_headers.push(("Transfer-Encoding".to_string(), "chunked".to_string()));

                // Compress the body
                let mut encoder = ZstdEncoder::new(Vec::new(), zstd_level)?;
                encoder.write_all(&buffer)?;
                let compressed = encoder.finish()?;
                final_size = compressed.len();

                // Send headers
                client.write_all(format!("{}\r\n", status_line).as_bytes())?;
                for (key, value) in &modified_headers {
                    client.write_all(format!("{}: {}\r\n", key, value).as_bytes())?;
                }
                client.write_all(b"\r\n")?;

                // Send compressed body
                let mut chunked_writer = BufWriter::new(&mut client);
                for chunk in compressed.chunks(8192) {
                    write!(chunked_writer, "{:X}\r\n", chunk.len())?;
                    chunked_writer.write_all(chunk)?;
                    write!(chunked_writer, "\r\n")?;
                }
                write!(chunked_writer, "0\r\n\r\n")?;
                chunked_writer.flush()
            } else {
                // No compression, forward as-is
                final_size = buffer.len();

                // Send headers with content length
                client.write_all(format!("{}\r\n", status_line).as_bytes())?;
                client.write_all(format!("Content-Length: {}\r\n", buffer.len()).as_bytes())?;
                for (key, value) in &modified_headers {
                    client.write_all(format!("{}: {}\r\n", key, value).as_bytes())?;
                }
                client.write_all(b"\r\n")?;

                // Send body
                client.write_all(&buffer)?;
                Ok(())
            }
        })
    };

    Ok((result, original_size, final_size))
}
