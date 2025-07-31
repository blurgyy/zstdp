use clap::Parser;
use std::fs;
use std::io;
use std::process;

mod args;
mod compression;
mod file_serving;
mod logging;
mod mime;
mod proxy;
mod server;

use args::{Args, CompressionPolicy};
use logging::setup_logging;
use mime::{MimeChecker, MimeConfig};
use server::start_server;

fn load_mime_types_from_file(path: &std::path::Path) -> io::Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect())
}

fn main() -> io::Result<()> {
    setup_logging();

    let args = Args::parse();

    // Handle MIME type listing options
    if args.list_compressible || args.list_non_compressible || args.list_unspecified {
        let mime_config = MimeConfig::default();
        let mime_checker = MimeChecker::new(mime_config).unwrap_or_else(|e| {
            eprintln!("Failed to initialize MIME database: {}", e);
            process::exit(1);
        });

        if args.list_compressible {
            let types = mime_checker.get_compressible_types();
            for mime_type in types {
                println!("{}", mime_type);
            }
        }

        if args.list_non_compressible {
            let types = mime_checker.get_non_compressible_types();
            for mime_type in types {
                println!("{}", mime_type);
            }
        }

        if args.list_unspecified {
            let types = mime_checker.get_unspecified_types();
            for mime_type in types {
                println!("{}", mime_type);
            }
        }

        return Ok(());
    }

    // Check that either forward or serve is provided for server mode
    if args.forward.is_none() && args.serve.is_none() {
        eprintln!("Error: Either --forward or --serve must be provided");
        process::exit(1);
    }

    // Build MIME configuration
    let mut additional_compressible = args.mime.clone();
    let mut excluded_types = args.mime_exclude.clone();

    // Load MIME types from files if specified
    if let Some(ref path) = args.mime_list_file {
        match load_mime_types_from_file(path) {
            Ok(types) => additional_compressible.extend(types),
            Err(e) => {
                eprintln!("Failed to read MIME list file {}: {}", path.display(), e);
                process::exit(1);
            }
        }
    }

    if let Some(ref path) = args.mime_list_exclude_file {
        match load_mime_types_from_file(path) {
            Ok(types) => excluded_types.extend(types),
            Err(e) => {
                eprintln!(
                    "Failed to read MIME exclude list file {}: {}",
                    path.display(),
                    e
                );
                process::exit(1);
            }
        }
    }

    let mime_config = MimeConfig {
        compression_policy: match args.compression_policy {
            CompressionPolicy::Strict => mime::CompressionPolicy::Strict,
            CompressionPolicy::Permissive => mime::CompressionPolicy::Permissive,
        },
        additional_compressible: additional_compressible.clone(),
        excluded_types,
        custom_list: if args.mime_list_file.is_some() {
            Some(additional_compressible)
        } else {
            None
        },
    };

    log::info!("Starting server with configuration:");
    log::info!("  Listen address: {}", args.listen_addr());
    log::info!("  Compression policy: {:?}", args.compression_policy);

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

    start_server(args, mime_config)
}
