# AGENTS.md - Development Guidelines for zstdp

## Build/Test Commands
- **Build**: `cargo build` (debug) / `cargo build --release` (production)
- **Test**: `cargo test` (no tests currently exist in this project)
- **Lint**: `cargo clippy` (check for common mistakes and style issues)
- **Format**: `cargo fmt` (format code according to Rust style guidelines)
- **Check**: `cargo check` (fast compile check without building binaries)
- **Important**: Always run `cargo clippy --fix --allow-dirty && cargo fmt` before committing changes
- **Outside `nix develop`**: the build needs `MIME_DB_PATH` (the embedded MIME db) and
  `JEMALLOC_OVERRIDE` (path to a prebuilt `libjemalloc.so`; otherwise `tikv-jemalloc-sys`
  tries to compile jemalloc itself and needs autoconf + make). The nix dev shell exports both.

## Code Style Guidelines
- **Language**: Rust 2021 edition
- **Imports**: Group `use` statements with std first, then external crates, then local modules
- **Error Handling**: Use `io::Result<T>` for I/O operations, `eprintln!` for user-facing errors, `log::` macros for debug output
- **Naming**: snake_case for functions/variables, PascalCase for types/enums, SCREAMING_SNAKE_CASE for constants
- **Types**: Explicit type annotations where needed, prefer `Option<T>` and `Result<T, E>` over panicking
- **Structure**: Organize code into modules (args, compression, file_serving, proxy, etc.)
- **Documentation**: Use doc comments (`///`) for public APIs
- **Logging**: Use `log::debug!`, `log::info!`, `log::trace!` for different verbosity levels
- **CLI Args**: Use `clap` with derive macros for argument parsing
- **No unsafe code**: This codebase avoids unsafe Rust

## Project Structure
- `src/main.rs` - Entry point and high-level orchestration
- `src/args.rs` - CLI argument definitions using clap
- `src/compression.rs` - Compression type definitions and logic
- `src/file_serving/` - Static file serving functionality
- `src/proxy/` - HTTP proxy functionality
- `src/mime.rs` - MIME type handling and compression policies