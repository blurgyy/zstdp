#[derive(Debug, PartialEq, Copy, Clone)]
pub enum CompressionType {
    Zstd,
    Gzip,
    None,
}

pub fn determine_compression(accept_encoding: &str) -> CompressionType {
    let binding = accept_encoding.to_lowercase();
    let encodings: Vec<&str> = binding.split(',').map(|s| s.trim()).collect();
    if encodings.iter().any(|&e| e == "zstd") {
        CompressionType::Zstd
    } else if encodings.iter().any(|&e| e == "gzip") {
        CompressionType::Gzip
    } else {
        CompressionType::None
    }
}
