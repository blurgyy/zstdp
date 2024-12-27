pub mod handlers;
mod path_utils;
pub mod spa;

use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
use mime_guess::from_path;
use percent_encoding::percent_decode_str;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use zstd::stream::write::Encoder as ZstdEncoder;

use crate::compression::{AcceptedCompression, CompressionType};
use crate::logging::LoggingExt;

pub struct PrecompressedFile {
    pub path: PathBuf,
    pub compression: CompressionType,
}

pub struct FileResponse {
    pub content: Vec<u8>,
    pub mime_type: String,
    pub compression: CompressionType,
    pub headers: Vec<(String, String)>,
}
