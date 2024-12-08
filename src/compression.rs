#[derive(Debug, PartialEq, Copy, Clone)]
pub enum CompressionType {
    Zstd,
    Gzip,
    None,
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct AcceptedCompression {
    pub supports_zstd: bool,
    pub supports_gzip: bool,
}

pub fn determine_compression(accept_encoding: &str) -> AcceptedCompression {
    let lowercase_ae = accept_encoding.to_lowercase();
    let encodings: Vec<&str> = lowercase_ae.split(',').map(|s| s.trim()).collect();

    AcceptedCompression {
        supports_zstd: encodings.iter().any(|&e| e == "zstd"),
        supports_gzip: encodings.iter().any(|&e| e == "gzip"),
    }
}
