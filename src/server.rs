use std::io::{self, BufRead, BufReader, ErrorKind};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use regex::Regex;

use crate::args::Args;
use crate::file_serving::handlers::handle_file_request;
use crate::logging::LoggingExt;
use crate::proxy::handlers::handle_proxy_connection;
use crate::{log_error, log_request, log_response};

pub fn start_server(args: Args) -> io::Result<()> {
    let listener = TcpListener::bind(args.listen_addr())?;
    log::info!("Server started on: {}", args.listen_addr());

    match (&args.forward, &args.serve) {
        (Some(addr), None) => log::info!("Mode: Proxy → {}", addr),
        (None, Some(dir)) => log::info!("Mode: File Server → {}", dir.display()),
        _ => unreachable!(),
    }

    let bypass_patterns = {
        let patterns: Result<Vec<Regex>, regex::Error> =
            args.bypass.iter().map(|p| Regex::new(p)).collect();

        match patterns {
            Ok(patterns) => {
                if !patterns.is_empty() {
                    log::info!("Loaded {} bypass patterns for compression", patterns.len());
                }
                Arc::new(patterns)
            }
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid bypass pattern: {}", e),
                ));
            }
        }
    };

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let args = args.clone();
                let bypass_patterns = Arc::clone(&bypass_patterns);
                thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &args, bypass_patterns) {
                        log_error!(e, "Connection handler failed");
                    }
                });
            }
            Err(e) => {
                log_error!(e, "Failed to accept connection");
            }
        }
    }

    Ok(())
}

fn handle_connection(
    client: TcpStream,
    args: &Args,
    bypass_patterns: Arc<Vec<Regex>>,
) -> io::Result<()> {
    let start_time = Instant::now();
    let peer_addr = client.peer_addr()?;
    log::debug!("→ New connection from {}", peer_addr);

    let result = match (&args.forward, &args.serve) {
        (Some(forward), None) => forward.log_operation("proxy_request", || {
            let request_time = Instant::now();
            let result = handle_proxy_connection(client, forward, args.zstd_level, bypass_patterns);

            match &result {
                Ok(_) => log_response!("200 OK", request_time.elapsed()),
                Err(_) => log_response!("500 Internal Server Error", request_time.elapsed()),
            }

            result
        }),
        (None, Some(serve)) => serve.log_operation("serve_files", || {
            let mut buf_reader = BufReader::new(&client);
            let mut first_line = String::new();
            buf_reader.read_line(&mut first_line)?;

            // Add request logging
            log_request!(&first_line);
            let request_time = Instant::now();

            let mut headers = Vec::new();
            let mut line = String::new();
            while {
                line.clear();
                buf_reader.read_line(&mut line)?;
                !line.trim().is_empty()
            } {
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    headers.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
                }
            }

            let result = handle_file_request(
                client,
                serve,
                &first_line,
                &headers,
                args.zstd_level,
                args.gzip_level,
                &bypass_patterns,
            );

            // Add response logging based on file existence
            match &result {
                Ok(_) => {
                    log_response!("200 OK", request_time.elapsed());
                    Ok(())
                }
                Err(e) => match e.kind() {
                    ErrorKind::NotFound => {
                        log_response!("404 Not Found", request_time.elapsed());
                        Ok(())
                    }
                    _ => {
                        log_response!("500 Internal Server Error", request_time.elapsed());
                        result
                    }
                },
            }
        }),
        _ => unreachable!(),
    };

    match result {
        Ok(_) => {
            log::debug!(
                "← Completed connection from {} in {:?}",
                peer_addr,
                start_time.elapsed()
            );
        }
        Err(e) => {
            log_error!(e, format!("Failed to handle connection from {}", peer_addr));
        }
    }

    Ok(())
}
