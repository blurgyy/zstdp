use super::*;
use crate::log_error;
use std::time::Instant;

pub fn sanitize_path(base_dir: &Path, request_path: &str) -> io::Result<Option<PathBuf>> {
    let start_time = Instant::now();
    log::debug!(
        "Sanitizing path - base: {}, request: {}",
        base_dir.display(),
        request_path
    );

    let canonical_base = base_dir.log_operation("canonicalize", || fs::canonicalize(base_dir))?;

    // Strip query parameters from the request path
    let path_without_query = request_path.split('?').next().unwrap_or(request_path);

    let decoded_path = request_path.log_operation("decode_path", || {
        percent_decode_str(path_without_query)
            .decode_utf8()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    })?;

    let cleaned_path = PathBuf::from(decoded_path.as_ref())
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .collect::<PathBuf>();
    log::debug!("Cleaned path: {}", cleaned_path.display());

    let requested_path = canonical_base.join(&cleaned_path);

    match fs::canonicalize(&requested_path) {
        Ok(path) => {
            log::debug!(
                "Path sanitization complete in {:?} - result: {}",
                start_time.elapsed(),
                path.display()
            );

            if path.starts_with(&canonical_base) {
                Ok(Some(path))
            } else {
                log::warn!("Path escapes base directory: {}", path.display());
                Ok(None)
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if requested_path.starts_with(&canonical_base) {
                log::debug!(
                    "Using non-canonicalized path (not found): {}",
                    requested_path.display()
                );
                Ok(Some(requested_path))
            } else {
                log::warn!("Non-existent path would escape base directory");
                Ok(None)
            }
        }
        Err(e) => {
            log_error!(
                e,
                format!("Failed to canonicalize path: {}", requested_path.display())
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
    let start_time = Instant::now();
    log::debug!("Looking for pre-compressed version of: {}", path.display());

    if !accepted_compression.supports_zstd && !accepted_compression.supports_gzip {
        log::debug!("No compression requested, skipping pre-compressed check");
        return Ok(None);
    }

    let canonical_base = base_dir.log_operation("canonicalize", || fs::canonicalize(base_dir))?;

    let rel_path = path.log_operation("strip_prefix", || {
        path.strip_prefix(&canonical_base)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    })?;

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
        log::debug!("Checking compressed path: {}", compressed_path.display());

        if compressed_path.exists() {
            let metadata = fs::metadata(&compressed_path)?;
            if metadata.is_file() {
                log::debug!(
                    "Found pre-compressed file ({:?}) in {:?}",
                    compression_type,
                    start_time.elapsed()
                );
                return Ok(Some(PrecompressedFile {
                    path: compressed_path,
                    compression: compression_type,
                }));
            }
        }
    }

    log::debug!("No pre-compressed file found in {:?}", start_time.elapsed());
    Ok(None)
}
