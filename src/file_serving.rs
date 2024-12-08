use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
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
    use super::*;

    pub fn sanitize_path(base_dir: &Path, request_path: &str) -> io::Result<Option<PathBuf>> {
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

    pub fn find_precompressed(
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
            if let Some(validated_path) =
                sanitize_path(base_dir, &compressed_path.to_string_lossy())?
            {
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
}

pub mod handlers {
    use path_utils::{find_precompressed, sanitize_path};

    use crate::compression::determine_compression;

    use super::*;
    use std::net::TcpStream;

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
