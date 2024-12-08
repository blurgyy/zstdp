use log::{debug, info};
use std::io::{self, BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::thread;

use crate::args::Args;
use crate::file_serving::handlers::handle_file_request;
use crate::proxy::handlers::handle_proxy_connection;

pub fn start_server(args: Args) -> io::Result<()> {
    let listener = TcpListener::bind(&args.listen_addr)?;
    println!("Listening on: {}", args.listen_addr);

    match (&args.forward_addr, &args.serve_dir) {
        (Some(addr), None) => println!("Forwarding to: {}", addr),
        (None, Some(dir)) => println!("Serving directory: {}", dir.display()),
        _ => unreachable!(),
    }

    for stream in listener.incoming() {
        let stream = stream?;
        let args = args.clone();

        thread::spawn(move || {
            if let Err(e) = handle_connection(stream, &args) {
                eprintln!("Error handling connection: {}", e);
            }
        });
    }

    Ok(())
}

fn handle_connection(client: TcpStream, args: &Args) -> io::Result<()> {
    info!("New connection received");

    match (&args.forward_addr, &args.serve_dir) {
        (Some(forward_addr), None) => {
            info!("Handling proxy request to {}", forward_addr);
            handle_proxy_connection(client, forward_addr, args.zstd_level)
        }
        (None, Some(serve_dir)) => {
            info!("Handling file serving request from {}", serve_dir.display());
            let mut buf_reader = BufReader::new(&client);
            let mut first_line = String::new();
            buf_reader.read_line(&mut first_line)?;
            info!("Request line: {}", first_line.trim());

            let mut headers = Vec::new();
            let mut line = String::new();
            while {
                line.clear();
                buf_reader.read_line(&mut line)?;
                !line.trim().is_empty()
            } {
                debug!("Header line: {}", line.trim());
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    headers.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
                }
            }

            handle_file_request(
                client,
                serve_dir,
                &first_line,
                &headers,
                args.zstd_level,
                args.gzip_level,
            )
        }
        _ => unreachable!(),
    }
}
