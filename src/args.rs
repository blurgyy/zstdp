use clap::{ArgGroup, Parser};
use regex::Regex;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
#[clap(group(ArgGroup::new("mode").required(true).args(&["forward", "serve"])))]
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
}

pub fn should_bypass_compression(uri: &str, bypass_patterns: &[Regex]) -> bool {
    log::trace!("{}", uri);
    bypass_patterns.iter().any(|pattern| pattern.is_match(uri))
}

impl Args {
    pub fn listen_addr(self: &Self) -> String {
        format!("{}:{}", self.bind, self.port)
    }
}
