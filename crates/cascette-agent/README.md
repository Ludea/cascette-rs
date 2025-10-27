# cascette-agent

Background HTTP service for managing product installations, updates, verification, and removal.

## Purpose

The cascette agent is a Battle.net Agent-compatible service that provides a REST API for:

- **Product Installation**: Install game products with progress tracking and resume support
- **Product Updates**: Update installed products with delta downloads
- **Installation Repair**: Verify and repair corrupted installations
- **Progress Monitoring**: Real-time operation progress with download metrics
- **Product Removal**: Uninstall products and free disk space

## Features

- **HTTP REST API**: Battle.net Agent-compatible endpoints on port 1120
- **State Persistence**: SQLite database for products, operations, and history
- **Resume Support**: Automatically resume interrupted operations on restart
- **Concurrent Operations**: Support 4+ concurrent operations (configurable)
- **Observability**: Structured logging, distributed tracing, Prometheus metrics
- **Security**: Localhost-only binding by default (configurable)

## Architecture

```text
┌─────────────┐
│ HTTP API    │  (axum, port 1120)
├─────────────┤
│ Operations  │  (Queue, Executor, State Machine)
├─────────────┤
│ State       │  (SQLite: products, operations, versions)
├─────────────┤
│ Installation│  (cascette-installation library)
└─────────────┘
```

## Usage

### Start Service

```bash
# Foreground
cascette-agent

# With configuration
cascette-agent --config /path/to/agent.toml

# With debug logging
RUST_LOG=debug cascette-agent
```

### API Examples

```bash
# Check agent status
curl http://localhost:1120/agent

# List products
curl http://localhost:1120/agent/products

# Install product
curl -X POST http://localhost:1120/agent/install/wow_classic \
  -H "Content-Type: application/json" \
  -d '{"install_path": "/games/wow", "region": "us", "locale": "enUS"}'

# Monitor operation
curl http://localhost:1120/agent/operations/{operation_id}
```

## Configuration

Example `agent.toml`:

```toml
[server]
port = 1120
fallback_ports = [6881, 6882, 6883]
bind_address = "127.0.0.1"
max_concurrent_operations = 4

[download]
max_connections_per_operation = 8
chunk_size_bytes = 1048576
retry_attempts = 3

[installation]
default_mode = "battlenet"
verify_after_install = true
resume_enabled = true
```

## Dependencies

- `axum`: HTTP server framework
- `tokio`: Async runtime
- `rusqlite`: SQLite database
- `tower`: HTTP middleware
- `tracing`: Structured logging
- `opentelemetry`: Distributed tracing
- `prometheus-client`: Metrics collection
- `cascette-installation`: Installation logic

## Development

```bash
# Build
cargo build --package cascette-agent

# Run
cargo run --package cascette-agent

# Test
cargo test --package cascette-agent
```

## License

MIT OR Apache-2.0
