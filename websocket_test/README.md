# WebSocket Bypass Testing

Simple test setup to verify zstdp's WebSocket bypassing functionality.

## Components

- `src/main.rs`: WebSocket echo server
- `client/index.html`: Browser client
- `Cargo.toml`: Dependencies

## Running Tests

1. Start WebSocket server:
   ```bash
   cargo run  # Starts on port 3012
   ```

2. Start two instances of zstdp:
   ```bash
   # Terminal 1: Proxy mode for WebSocket
   zstdp -b 127.0.0.1 -p 9866 -f 127.0.0.1:3012

   # Terminal 2: File server for client
   zstdp -s client/ -p 8000
   ```

3. Open `http://localhost:8000` in browser
   - Type messages and see them echoed back
   - Check zstdp logs for WebSocket tunnel creation

## Troubleshooting

- Verify all ports (3012, 9866, 8000) are available
- Check browser DevTools Network tab for WebSocket connection status
- Look for any errors in zstdp proxy logs
