use clap::Parser;
use std::io;

mod args;
mod compression;
mod file_serving;
mod proxy;
mod server;

use args::Args;
use server::start_server;

fn main() -> io::Result<()> {
    let args = Args::parse();
    start_server(args)
}
