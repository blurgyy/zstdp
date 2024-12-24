use path_utils::{find_precompressed, sanitize_path};

use crate::compression::{determine_compression, AcceptedCompression};

use super::*;
use std::net::TcpStream;

pub fn serve_file(
    base_dir: &Path,
    request_path: &str,
    accepted_compression: AcceptedCompression,
    zstd_level: i32,
    gzip_level: u32,
) -> io::Result<Option<FileResponse>> {
    log::debug!("Received request for path: {}", request_path);
    log::trace!("Base directory: {}", base_dir.display());
    log::trace!(
        "Accepted compression - zstd: {}, gzip: {}",
        accepted_compression.supports_zstd,
        accepted_compression.supports_gzip
    );

    let path = match sanitize_path(base_dir, request_path)? {
        Some(p) => {
            log::debug!("Sanitized path: {}", p.display());
            p
        }
        None => {
            log::warn!("Path sanitization failed or path is outside base directory");
            return Ok(None);
        }
    };

    let final_path = if path.is_dir() {
        log::debug!("Path is a directory, looking for index.html");
        path.join("index.html")
    } else {
        path
    };

    log::debug!("Final resolved path: {}", final_path.display());

    // First try to find any pre-compressed version
    if let Some(precompressed) = find_precompressed(base_dir, &final_path, accepted_compression)? {
        log::debug!(
            "Using pre-compressed file: {} with compression {:?}",
            precompressed.path.display(),
            precompressed.compression
        );

        let mut content = Vec::new();
        File::open(&precompressed.path)?.read_to_end(&mut content)?;

        let mime_type = from_path(&final_path).first_or_octet_stream().to_string();

        return Ok(Some(FileResponse {
            content,
            mime_type,
            compression: precompressed.compression,
        }));
    }

    // If no pre-compressed file exists, check if original file exists
    if !final_path.exists() {
        log::warn!("File not found: {}", final_path.display());
        return Ok(None);
    }

    let metadata = fs::metadata(&final_path)?;
    if !metadata.is_file() {
        log::warn!(
            "Path exists but is not a regular file: {}",
            final_path.display()
        );
        return Ok(None);
    }

    // Read original file
    let mut content = Vec::new();
    File::open(&final_path)?.read_to_end(&mut content)?;

    let mime_type = from_path(&final_path).first_or_octet_stream().to_string();

    // Compress if needed
    let (final_content, compression) = if accepted_compression.supports_zstd {
        log::debug!("Compressing with zstd level {}", zstd_level);
        let mut encoder = ZstdEncoder::new(Vec::new(), zstd_level)?;
        encoder.write_all(&content)?;
        (encoder.finish()?, CompressionType::Zstd)
    } else if accepted_compression.supports_gzip {
        log::debug!("Compressing with gzip level {}", gzip_level);
        let mut encoder = GzEncoder::new(Vec::new(), GzipCompression::new(gzip_level));
        encoder.write_all(&content)?;
        (encoder.finish()?, CompressionType::Gzip)
    } else {
        (content, CompressionType::None)
    };

    Ok(Some(FileResponse {
        content: final_content,
        mime_type,
        compression,
    }))
}

pub fn handle_file_request(
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
