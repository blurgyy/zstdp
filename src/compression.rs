use std::fmt;
use std::io::{self, Write};

use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
use zstd::stream::write::Encoder as ZstdEncoder;

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

impl AcceptedCompression {
    /// The coding to respond with: zstd whenever it is acceptable (regardless of
    /// the relative q-values), else gzip, else identity.
    pub fn preferred(&self) -> CompressionType {
        if self.supports_zstd {
            CompressionType::Zstd
        } else if self.supports_gzip {
            CompressionType::Gzip
        } else {
            CompressionType::None
        }
    }
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

/// The highest q-value an `Accept-Encoding` element list gives to any of `codings`
/// (lowercase), or `None` if none of them is listed.
///
/// Elements are `coding *( OWS ";" OWS parameter )` per RFC 9110 12.5.3; a missing
/// `q` means 1, and an element whose `q` is not a number is ignored.
fn listed_qvalue(accept_encoding: &str, codings: &[&str]) -> Option<f32> {
    accept_encoding
        .split(',')
        .filter_map(|element| {
            let mut parts = element.split(';');
            let coding = parts.next()?.trim().to_ascii_lowercase();
            if !codings.contains(&coding.as_str()) {
                return None;
            }
            let mut qvalue = 1.0;
            for param in parts {
                let Some((name, value)) = param.split_once('=') else {
                    continue;
                };
                if name.trim().eq_ignore_ascii_case("q") {
                    match value.trim().parse::<f32>() {
                        Ok(q) if q.is_finite() => qvalue = q,
                        Ok(_) | Err(_) => {
                            log::debug!("Ignoring Accept-Encoding element with bad q: {element}");
                            return None;
                        }
                    }
                }
            }
            Some(qvalue)
        })
        .reduce(f32::max)
}

/// Whether a content coding is acceptable per RFC 9110 12.5.3: listed (under any of
/// its `codings` names) with q > 0, or not listed while `*` is listed with q > 0.
fn is_acceptable(accept_encoding: &str, codings: &[&str]) -> bool {
    match listed_qvalue(accept_encoding, codings) {
        Some(q) => q > 0.0,
        None => listed_qvalue(accept_encoding, &["*"]).is_some_and(|q| q > 0.0),
    }
}

pub fn determine_compression(accept_encoding: &str) -> AcceptedCompression {
    let compression = AcceptedCompression {
        supports_zstd: is_acceptable(accept_encoding, &["zstd"]),
        // RFC 9110 8.4.1.3: x-gzip is equivalent to gzip.
        supports_gzip: is_acceptable(accept_encoding, &["gzip", "x-gzip"]),
    };

    log::debug!("Determined compression support from '{accept_encoding}': {compression}");

    compression
}

/// Encodes `data` with `compression`; `CompressionType::None` returns it unchanged.
pub fn compress(
    data: Vec<u8>,
    compression: CompressionType,
    zstd_level: i32,
    gzip_level: u32,
) -> io::Result<Vec<u8>> {
    match compression {
        CompressionType::Zstd => {
            log::debug!("Compressing with zstd level {zstd_level}");
            let mut encoder = ZstdEncoder::new(Vec::new(), zstd_level)?;
            encoder.write_all(&data)?;
            encoder.finish()
        }
        CompressionType::Gzip => {
            log::debug!("Compressing with gzip level {gzip_level}");
            let mut encoder = GzEncoder::new(Vec::new(), GzipCompression::new(gzip_level));
            encoder.write_all(&data)?;
            encoder.finish()
        }
        CompressionType::None => Ok(data),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn preferred(accept_encoding: &str) -> CompressionType {
        determine_compression(accept_encoding).preferred()
    }

    #[test]
    fn browser_headers() {
        assert_eq!(preferred("gzip, deflate, br"), CompressionType::Gzip);
        assert_eq!(preferred("gzip, deflate, br, zstd"), CompressionType::Zstd);
        assert_eq!(preferred("identity"), CompressionType::None);
        assert_eq!(preferred(""), CompressionType::None);
    }

    #[test]
    fn tokens_are_case_insensitive_and_may_carry_parameters() {
        assert_eq!(preferred("ZSTD;q=1.0"), CompressionType::Zstd);
        assert_eq!(preferred("GZip ; Q=0.8"), CompressionType::Gzip);
        assert_eq!(preferred("x-gzip"), CompressionType::Gzip);
    }

    #[test]
    fn zero_q_is_not_acceptable() {
        assert_eq!(preferred("zstd;q=0, gzip"), CompressionType::Gzip);
        assert_eq!(preferred("gzip;q=0"), CompressionType::None);
        assert_eq!(preferred("zstd;q=0.000, gzip;q=0"), CompressionType::None);
    }

    #[test]
    fn zstd_wins_whenever_acceptable() {
        assert_eq!(preferred("gzip;q=1, zstd;q=0.1"), CompressionType::Zstd);
    }

    #[test]
    fn wildcard_covers_unlisted_codings_only() {
        assert_eq!(preferred("*"), CompressionType::Zstd);
        assert_eq!(preferred("zstd;q=0, *"), CompressionType::Gzip);
        assert_eq!(preferred("gzip, *;q=0"), CompressionType::Gzip);
        assert_eq!(preferred("*;q=0"), CompressionType::None);
    }

    #[test]
    fn malformed_q_is_ignored() {
        assert_eq!(preferred("zstd;q=abc, gzip"), CompressionType::Gzip);
        assert_eq!(preferred("zstd;q=NaN"), CompressionType::None);
    }

    #[test]
    fn compress_round_trips() {
        let data = b"hello hello hello hello".repeat(100);

        let gz = compress(data.clone(), CompressionType::Gzip, 3, 6).unwrap();
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(&gz[..])
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, data);

        let zst = compress(data.clone(), CompressionType::Zstd, 3, 6).unwrap();
        assert_eq!(zstd::decode_all(&zst[..]).unwrap(), data);

        assert_eq!(
            compress(data.clone(), CompressionType::None, 3, 6).unwrap(),
            data
        );
    }
}
