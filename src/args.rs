use clap::{ArgGroup, Parser, ValueEnum};
use regex::Regex;
use std::path::PathBuf;

#[derive(Clone, Debug, ValueEnum)]
pub enum CompressionPolicy {
    /// Only compress MIME types explicitly marked as compressible
    #[value(help = "Only compress MIME types explicitly marked as compressible")]
    Strict,
    /// Compress all MIME types except those explicitly marked as non-compressible
    #[value(help = "Compress all MIME types except those explicitly marked as non-compressible")]
    Permissive,
}

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
#[clap(group(ArgGroup::new("mode").args(&["forward", "serve"])))]
pub struct Args {
    #[arg(short, long, default_value = "127.0.0.1")]
    pub bind: String,

    #[arg(short, long, default_value = "9866")]
    pub port: u16,

    #[arg(short, long)]
    pub forward: Option<String>,

    #[arg(short, long)]
    pub serve: Option<PathBuf>,

    #[arg(short, long, default_value = "3")]
    pub zstd_level: i32,

    #[arg(short, long, default_value = "6")]
    pub gzip_level: u32,

    #[arg(short = 'i', long, action = clap::ArgAction::Append)]
    pub bypass: Vec<String>,

    #[arg(long)]
    pub spa: bool,

    /// File containing MIME types to compress (one per line, # for comments)
    #[arg(
        long,
        help = "Load custom list of MIME types to compress from file (one per line, # for comments)"
    )]
    pub mime_list_file: Option<PathBuf>,

    /// File containing MIME types to exclude from compression (one per line, # for comments)
    #[arg(
        long,
        help = "Load list of MIME types to exclude from compression from file (one per line, # for comments)"
    )]
    pub mime_list_exclude_file: Option<PathBuf>,

    /// Add individual MIME types to compression list (can be used multiple times)
    #[arg(long, action = clap::ArgAction::Append, help = "Add a MIME type to the compression list (can be used multiple times)")]
    pub mime: Vec<String>,

    /// Exclude individual MIME types from compression (can be used multiple times)
    #[arg(long, action = clap::ArgAction::Append, help = "Exclude a MIME type from compression (can be used multiple times)")]
    pub mime_exclude: Vec<String>,

    /// How to handle MIME types with unknown compressibility: 'strict' = compress only known-good types, 'permissive' = compress unless known-bad
    #[arg(
        long,
        value_enum,
        default_value = "strict",
        help = "How to handle MIME types with unknown compressibility",
        long_help = "Control how to handle MIME types that are not explicitly marked as compressible or non-compressible in the database:\n\
                      'strict' (default): Only compress MIME types explicitly marked as compressible\n\
                      'permissive': Compress all MIME types except those explicitly marked as non-compressible"
    )]
    pub compression_policy: CompressionPolicy,

    /// List all MIME types marked as compressible in the embedded database
    #[arg(
        long,
        help = "List all MIME types marked as compressible in the embedded mime-db database"
    )]
    pub list_compressible: bool,

    /// List all MIME types marked as non-compressible in the embedded database
    #[arg(
        long,
        help = "List all MIME types marked as non-compressible in the embedded mime-db database"
    )]
    pub list_non_compressible: bool,

    /// List all MIME types with unspecified compressibility in the embedded database
    #[arg(
        long,
        help = "List all MIME types with unspecified compressibility (handled according to --compression-policy)"
    )]
    pub list_unspecified: bool,
}

pub fn should_bypass_compression(uri: &str, bypass_patterns: &[Regex]) -> bool {
    log::trace!("{uri}");
    bypass_patterns.iter().any(|pattern| pattern.is_match(uri))
}

impl Args {
    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.bind, self.port)
    }
}
