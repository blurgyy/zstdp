# zstdp

A versatile HTTP server that can function both as a proxy server and a file server, with advanced
compression support (Zstd and Gzip) and various optimization features.

## Features

- **Dual Mode Operation**:
  - Proxy Mode: Forward requests to a backend server with optional compression
  - File Server Mode: Serve static files from a local directory

- **Advanced Compression**:
  - Zstd compression support with configurable compression levels in both modes
  - Gzip compression support with configurable compression levels (file server mode)
  - Content-aware compression with configurable bypass patterns using regex
  - Pre-compressed file support (.zst and .gz)

- **File Serving Features**:
  - Single Page Application (SPA) support with configurable routing
  - Automatic index.html serving for directories
  - Intelligent cache control headers
  - Security headers included by default
  - Path sanitization and security checks

- **Proxy Features**:
  - Transparent proxying with compression
  - Chunked transfer encoding support
  - Header manipulation and forwarding
  - Custom compression decisions based on content
  - WebSocket connection pass-through (uncompressed)

- **General Features**:
  - Auto-detected colorized logging with configurable levels
  - Detailed request/response logging with performance metrics
  - Multi-threaded request handling
  - Terminal and non-terminal aware output formatting

## Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/blurgyy/zstdp.git
   cd zstdp
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

## Usage

The server can be run in either proxy mode or file server mode:

### Proxy Mode

```bash
zstdp -b 127.0.0.1 -p 9866 -f backend-server:8080
```

### File Server Mode

```bash
zstdp -b 127.0.0.1 -p 9866 -s ./path/to/files
```

### Command Line Options

```
Options:
  -b, --bind <ADDR>          Bind address [default: 127.0.0.1]
  -p, --port <PORT>          Port number [default: 9866]
  -f, --forward <URL>        Forward requests to specified URL (proxy mode)
  -s, --serve <PATH>         Serve files from directory (file server mode)
  -z, --zstd-level <LEVEL>   Zstd compression level [default: 3]
  -g, --gzip-level <LEVEL>   Gzip compression level [default: 6]
  -i, --bypass <PATTERN>     Regex patterns to bypass compression
      --spa                  Enable SPA mode (serves index.html for non-file routes)
  -h, --help                 Print help
  -V, --version             Print version
```

## WebSocket Support

WebSocket connections are automatically detected and passed through the proxy without compression.
This ensures compatibility with WebSocket-based applications while maintaining compression benefits
for regular HTTP traffic.  See `websocket_test/` directory for testing examples.

## Security Features

- Path traversal prevention through path sanitization
- Proper MIME type detection and handling
- URL sanitization and validation

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open
an issue first to discuss what you would like to change.

## License

Licensed under the Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0).
Files in this repository may not be copied, modified, or distributed except according to those
terms.

## Acknowledgments

This project was developed with the assistance of Claude (Anthropic). Both the codebase and
documentation were created through collaborative discussion with Claude 3.5 Sonnet, which provided
guidance on implementation details, feature design, and documentation.
