use clap::{ArgGroup, Parser};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
#[clap(group(ArgGroup::new("mode").required(true).args(&["forward_addr", "serve_dir"])))]
pub struct Args {
    #[arg(short, long)]
    pub listen_addr: String,

    #[arg(short, long)]
    pub forward_addr: Option<String>,

    #[arg(short, long)]
    pub serve_dir: Option<PathBuf>,

    #[arg(short, long, default_value = "3")]
    pub zstd_level: i32,

    #[arg(short, long, default_value = "6")]
    pub gzip_level: u32,
}
