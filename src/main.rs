use clap::Parser;
use std::io;

mod args;
mod compression;
mod file_serving;
mod logging;
mod proxy;
mod server;

use args::Args;
use logging::setup_logging;
use server::start_server;

fn main() -> io::Result<()> {
    setup_logging();

    let args = Args::parse();
    log::info!("Starting server with configuration:");
    log::info!("  Listen address: {}", args.listen_addr());

    if let Some(addr) = &args.forward {
        log::info!("  Mode: Proxy");
        log::info!("  Forward address: {}", addr);
        log::info!("  Zstd compression level: {}", args.zstd_level);
    } else if let Some(dir) = &args.serve {
        log::info!("  Mode: File Server");
        log::info!("  Serving directory: {}", dir.display());
        log::info!(
            "  Compression levels - Zstd: {}, Gzip: {}",
            args.zstd_level,
            args.gzip_level
        );
    }

    start_server(args)
}
