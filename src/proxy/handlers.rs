use io::BufWriter;
use regex::Regex;
use transfer::tunnel_connection;

use crate::args::should_bypass_compression;
use crate::compression::{compress, CompressionType};
use crate::logging::LoggingExt;
use crate::mime::MimeChecker;

use super::headers::{add_vary_accept_encoding, parse_response_headers, status_code, weaken_etag};
use super::transfer::{
    forward_chunked_body, forward_request, read_chunked_body, read_response_head,
};
use super::*;
use std::sync::Arc;
use std::time::Instant;

/// Writes a response head built from a status line and (possibly modified) headers.
fn write_head<W: Write>(
    writer: &mut W,
    status_line: &str,
    headers: &[(String, String)],
) -> io::Result<()> {
    writer.write_all(format!("{status_line}\r\n").as_bytes())?;
    for (key, value) in headers {
        writer.write_all(format!("{key}: {value}\r\n").as_bytes())?;
    }
    writer.write_all(b"\r\n")
}

pub fn handle_proxy_connection(
    mut client: TcpStream,
    forward: &str,
    zstd_level: i32,
    gzip_level: u32,
    bypass_patterns: Arc<Vec<Regex>>,
    mime_checker: Arc<MimeChecker>,
) -> io::Result<(io::Result<()>, usize, usize)> {
    let start_time = Instant::now();
    log::debug!("→ New proxy connection to {forward}");

    let mut server = TcpStream::connect(forward).map_err(|e| {
        log::error!("Failed to connect to backend {forward}: {e}");
        e
    })?;
    log::debug!("Connected to backend server in {:?}", start_time.elapsed());

    let request = forward.log_operation("forward_request", || {
        forward_request(&mut client, &mut server.try_clone()?)
    })?;

    // Check for WebSocket upgrade request
    let is_websocket = request
        .headers
        .iter()
        .any(|(k, v)| k.to_lowercase() == "upgrade" && v.to_lowercase().contains("websocket"));

    if is_websocket {
        log::debug!("WebSocket upgrade request detected, creating tunnel");
        // For WebSocket connections, create a direct tunnel
        return Ok((tunnel_connection(client, server), 0, 0));
    }

    let should_bypass = should_bypass_compression(&request.uri, &bypass_patterns);
    if should_bypass {
        log::debug!(
            "URI '{}' matches bypass pattern, skipping compression",
            request.uri
        );
    }

    // Read the final response head, relaying interim (1xx) responses as they come.
    let response_headers = loop {
        let head = read_response_head(&mut server)?;
        let status_line = parse_response_headers(&String::from_utf8_lossy(&head))
            .0
            .to_string();
        match status_code(&status_line) {
            Some(101) => {
                log::debug!("← {status_line} from backend, creating tunnel");
                client.write_all(&head)?;
                return Ok((tunnel_connection(client, server), 0, 0));
            }
            Some(100..=199) => {
                log::debug!("← {status_line} (interim) from backend");
                client.write_all(&head)?;
            }
            Some(_) | None => break head,
        }
    };

    let response_headers_str = String::from_utf8_lossy(&response_headers).to_string();
    let (status_line, headers) = parse_response_headers(&response_headers_str);
    log::debug!("← {status_line} from backend");
    let status = status_code(status_line);

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

    // Check MIME type for compression
    let content_type = headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "content-type")
        .map(|(_, v)| v.split(';').next().unwrap_or(v).trim());

    let should_compress_mime = content_type
        .map(|ct| mime_checker.should_compress(ct))
        .unwrap_or(false);

    if let Some(content_type) = content_type {
        log::debug!("Content-Type: {content_type}, should compress: {should_compress_mime}");
    }

    // Server-sent events must reach the client as they are produced: never buffer
    // them for compression, whatever the MIME configuration says.
    let is_event_stream =
        content_type.is_some_and(|ct| ct.eq_ignore_ascii_case("text/event-stream"));

    let is_eligible =
        !is_already_compressed && !should_bypass && should_compress_mime && !is_event_stream;

    // RFC 9112 6.3: responses to HEAD, and 204/304 responses, never carry content,
    // whatever their Content-Length or Transfer-Encoding say.
    let is_bodiless =
        request.method.eq_ignore_ascii_case("HEAD") || matches!(status, Some(204 | 304));

    let encoding = request.accepted_compression.preferred();

    log::debug!(
        "Response properties - compressed: {is_already_compressed}, chunked: {is_chunked}, length: {content_length:?}, eligible: {is_eligible}, bodiless: {is_bodiless}, encoding: {encoding}"
    );

    let mut original_size = 0;
    let mut final_size = 0;

    let result = if is_bodiless {
        forward.log_operation("forward_bodiless", || {
            if is_eligible {
                let mut modified_headers = headers.clone();
                add_vary_accept_encoding(&mut modified_headers);
                // A 304 stands in for the response zstdp would have encoded, so it
                // carries that response's validator.
                if status == Some(304) && encoding != CompressionType::None {
                    weaken_etag(&mut modified_headers);
                }
                write_head(&mut client, status_line, &modified_headers)
            } else {
                client.write_all(&response_headers)
            }
        })
    } else if !is_eligible {
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
                original_size = read_chunked_body(&mut server, &mut buffer)?;
            } else if let Some(length) = content_length {
                io::copy(&mut server.take(length as u64), &mut buffer)?;
                original_size = length;
            } else {
                let bytes = io::copy(&mut server, &mut buffer)?;
                original_size = bytes as usize;
            }

            // The body is re-framed below, so upstream's framing no longer applies.
            let mut modified_headers = headers.clone();
            modified_headers.retain(|(k, _)| {
                k.to_lowercase() != "content-length" && k.to_lowercase() != "transfer-encoding"
            });
            add_vary_accept_encoding(&mut modified_headers);

            match encoding {
                CompressionType::Zstd | CompressionType::Gzip => {
                    let compressed = compress(buffer, encoding, zstd_level, gzip_level)?;
                    final_size = compressed.len();

                    weaken_etag(&mut modified_headers);
                    modified_headers.push(("Content-Encoding".to_string(), encoding.to_string()));
                    modified_headers.push(("Transfer-Encoding".to_string(), "chunked".to_string()));
                    write_head(&mut client, status_line, &modified_headers)?;

                    // Send compressed body
                    let mut chunked_writer = BufWriter::new(&mut client);
                    for chunk in compressed.chunks(8192) {
                        write!(chunked_writer, "{:X}\r\n", chunk.len())?;
                        chunked_writer.write_all(chunk)?;
                        write!(chunked_writer, "\r\n")?;
                    }
                    write!(chunked_writer, "0\r\n\r\n")?;
                    chunked_writer.flush()
                }
                CompressionType::None => {
                    // No compression, forward as-is
                    final_size = buffer.len();

                    // Send headers with content length
                    client.write_all(format!("{status_line}\r\n").as_bytes())?;
                    client.write_all(format!("Content-Length: {}\r\n", buffer.len()).as_bytes())?;
                    for (key, value) in &modified_headers {
                        client.write_all(format!("{key}: {value}\r\n").as_bytes())?;
                    }
                    client.write_all(b"\r\n")?;

                    // Send body
                    client.write_all(&buffer)?;
                    Ok(())
                }
            }
        })
    };

    Ok((result, original_size, final_size))
}
