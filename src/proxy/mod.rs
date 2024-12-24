pub mod handlers;
pub mod headers;
pub mod transfer;

use std::io::{self, Read, Write};
use std::net::TcpStream;
use zstd::stream::write::Encoder as ZstdEncoder;
