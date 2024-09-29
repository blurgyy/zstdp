# zstdp

A simple HTTP proxy server that compresses HTTP responses using Zstd if the client supports it.
It forwards requests to a specified backend server and compresses responses with Zstd when
requested.

## Usage

### Installation

1. Clone the repository:

   ```bash
   git clone https://github.com/your-username/zstdp.git
   cd zstdp
   ```

2. Build the project:

   ```bash
   cargo build --release
   ```

3. Run the compiled binary:

   ```bash
   ./target/release/zstdp --listen-addr <LISTEN_ADDRESS> --forward-addr <FORWARD_ADDRESS> [OPTIONS]
   ```

### Command-Line Arguments

- `-l|--listen-addr` (required): Address to bind the proxy server to (e.g., `127.0.0.1:8080`).
- `-f|--forward-addr` (required): Address of the backend server to forward requests to (e.g., `127.0.0.1:80`).
- `-z|--zstd-level` (optional): Compression level for Zstd (default: `3`).
- `--custom-header` (optional): Add a custom header to all forwarded requests.

### Example Usage

Start the proxy server to listen on `127.0.0.1:8080` and forward requests to a backend server at `127.0.0.1:80`:

```bash
./target/release/zstdp --listen-addr 127.0.0.1:8080 --forward-addr 127.0.0.1:80
```

Add a custom header and specify a Zstd compression level of `5`:

```bash
./target/release/zstdp --listen-addr 127.0.0.1:8080 --forward-addr 127.0.0.1:80 --custom-header "X-Proxy-Header: ZstdProxy" --zstd-level 5
```

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.

## License

Licensed under the Apache License, Version 2.0 http://www.apache.org/licenses/LICENSE-2.0.
The files in this repository may not be copied, modified, or distributed except according to those
terms.

## Disclaimer

The initial version of this project was created with the help of `claude-3.5-sonnet` and
`im-also-a-good-gpt2-chatbot`.
