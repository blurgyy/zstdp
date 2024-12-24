use std::fmt;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum CompressionType {
    Zstd,
    Gzip,
    None,
}

impl fmt::Display for CompressionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompressionType::Zstd => write!(f, "zstd"),
            CompressionType::Gzip => write!(f, "gzip"),
            CompressionType::None => write!(f, "none"),
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct AcceptedCompression {
    pub supports_zstd: bool,
    pub supports_gzip: bool,
}

impl fmt::Display for AcceptedCompression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "zstd: {}, gzip: {}",
            self.supports_zstd, self.supports_gzip
        )
    }
}

pub fn determine_compression(accept_encoding: &str) -> AcceptedCompression {
    let lowercase_ae = accept_encoding.to_lowercase();
    let encodings: Vec<&str> = lowercase_ae.split(',').map(|s| s.trim()).collect();

    let compression = AcceptedCompression {
        supports_zstd: encodings.iter().any(|&e| e == "zstd"),
        supports_gzip: encodings.iter().any(|&e| e == "gzip"),
    };

    log::debug!(
        "Determined compression support from '{}': {}",
        accept_encoding,
        compression
    );

    compression
}
