use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
use log::{debug, info, warn};
use mime_guess::from_path;
use percent_encoding::percent_decode_str;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use zstd::stream::write::Encoder as ZstdEncoder;

use crate::compression::CompressionType;

pub struct PrecompressedFile {
    pub path: PathBuf,
    pub compression: CompressionType,
}

pub struct FileResponse {
    pub content: Vec<u8>,
    pub mime_type: String,
    pub compression: CompressionType,
}

mod path_utils {
    use crate::compression::AcceptedCompression;

    use super::*;

    pub fn sanitize_path(base_dir: &Path, request_path: &str) -> io::Result<Option<PathBuf>> {
        debug!(
            "Sanitizing path - base_dir: {}, request_path: {}",
            base_dir.display(),
            request_path
        );

        let canonical_base = match fs::canonicalize(base_dir) {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    "Failed to canonicalize base directory {}: {}",
                    base_dir.display(),
                    e
                );
                return Err(e);
            }
        };
        debug!("Canonical base dir: {}", canonical_base.display());

        // Strip query parameters from the request path
        let path_without_query = request_path.split('?').next().unwrap_or(request_path);
        debug!("Path without query parameters: {}", path_without_query);

        let decoded_path = match percent_decode_str(path_without_query).decode_utf8() {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to decode path {}: {}", path_without_query, e);
                return Err(io::Error::new(io::ErrorKind::InvalidData, e));
            }
        };
        debug!("Decoded request path: {}", decoded_path);

        let cleaned_path = PathBuf::from(decoded_path.as_ref())
            .components()
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .collect::<PathBuf>();
        debug!("Cleaned path components: {}", cleaned_path.display());

        let requested_path = canonical_base.join(&cleaned_path);
        debug!("Combined path: {}", requested_path.display());

        match fs::canonicalize(&requested_path) {
            Ok(path) => {
                debug!("Successfully canonicalized to: {}", path.display());
                if path.starts_with(&canonical_base) {
                    debug!("Path is within base directory");
                    Ok(Some(path))
                } else {
                    warn!("Path escapes base directory: {}", path.display());
                    Ok(None)
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                debug!(
                    "Path doesn't exist, using non-canonicalized: {}",
                    requested_path.display()
                );
                if requested_path.starts_with(&canonical_base) {
                    Ok(Some(requested_path))
                } else {
                    warn!("Non-existent path would escape base directory");
                    Ok(None)
                }
            }
            Err(e) => {
                warn!(
                    "Error canonicalizing path {}: {}",
                    requested_path.display(),
                    e
                );
                Err(e)
            }
        }
    }

    pub fn find_precompressed(
        base_dir: &Path,
        path: &Path,
        accepted_compression: AcceptedCompression,
    ) -> io::Result<Option<PrecompressedFile>> {
        info!("Checking for pre-compressed version of: {}", path.display());
        debug!("Base directory: {}", base_dir.display());
        debug!(
            "Accepted compression - zstd: {}, gzip: {}",
            accepted_compression.supports_zstd, accepted_compression.supports_gzip
        );

        if !accepted_compression.supports_zstd && !accepted_compression.supports_gzip {
            debug!("No compression requested, skipping pre-compressed file check");
            return Ok(None);
        }

        let canonical_base = fs::canonicalize(base_dir)?;
        debug!("Canonical base dir: {}", canonical_base.display());

        let rel_path = path
            .strip_prefix(&canonical_base)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        debug!("Relative path: {}", rel_path.display());

        // Try all supported compression types in order of preference
        let mut possible_compressions = Vec::new();
        if accepted_compression.supports_zstd {
            possible_compressions.push((CompressionType::Zstd, ".zst"));
        }
        if accepted_compression.supports_gzip {
            possible_compressions.push((CompressionType::Gzip, ".gz"));
        }

        // Check each possible compression type
        for (compression_type, extension) in possible_compressions {
            let compressed_path =
                canonical_base.join(Path::new(&format!("{}{}", rel_path.display(), extension)));
            debug!("Checking compressed path: {}", compressed_path.display());

            if compressed_path.exists() {
                let metadata = fs::metadata(&compressed_path)?;
                if metadata.is_file() {
                    info!(
                        "Found valid pre-compressed file: {} with {:?}",
                        compressed_path.display(),
                        compression_type
                    );
                    return Ok(Some(PrecompressedFile {
                        path: compressed_path,
                        compression: compression_type,
                    }));
                }
            }
        }

        info!(
            "No suitable pre-compressed file found for: {}",
            path.display()
        );
        Ok(None)
    }
}

pub mod handlers {
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
        info!("Received request for path: {}", request_path);
        debug!("Base directory: {}", base_dir.display());
        debug!(
            "Accepted compression - zstd: {}, gzip: {}",
            accepted_compression.supports_zstd, accepted_compression.supports_gzip
        );

        let path = match sanitize_path(base_dir, request_path)? {
            Some(p) => {
                info!("Sanitized path: {}", p.display());
                p
            }
            None => {
                warn!("Path sanitization failed or path is outside base directory");
                return Ok(None);
            }
        };

        let final_path = if path.is_dir() {
            info!("Path is a directory, looking for index.html");
            path.join("index.html")
        } else {
            path
        };

        info!("Final resolved path: {}", final_path.display());

        // First try to find any pre-compressed version
        if let Some(precompressed) =
            find_precompressed(base_dir, &final_path, accepted_compression)?
        {
            info!(
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
            warn!("File not found: {}", final_path.display());
            return Ok(None);
        }

        let metadata = fs::metadata(&final_path)?;
        if !metadata.is_file() {
            warn!(
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
            info!("Compressing with zstd level {}", zstd_level);
            let mut encoder = ZstdEncoder::new(Vec::new(), zstd_level)?;
            encoder.write_all(&content)?;
            (encoder.finish()?, CompressionType::Zstd)
        } else if accepted_compression.supports_gzip {
            info!("Compressing with gzip level {}", gzip_level);
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

                client.write_all(
                    format!("Content-Length: {}\r\n", response.content.len()).as_bytes(),
                )?;
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
}
