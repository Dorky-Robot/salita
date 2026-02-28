# Salita

A home device mesh with MCP interface. Every machine runs a node, they auto-discover each other via mDNS, and AI agents connect via MCP to browse, search, and read files across all devices.

## How It Works

```
Claude Code ──(stdio/MCP)──► salita mcp ──► local filesystem
                                         ──► local SQLite DB
                                         ──► peer HTTP APIs

salita serve (daemon on each machine):
  - HTTP server for peer file access
  - mDNS registration + discovery
```

Two modes: `salita serve` (HTTP daemon + mDNS) and `salita mcp` (MCP stdio server).

## Quick Start

```bash
cargo install --path .

# Configure directories to expose
mkdir -p ~/.salita
cp config.example.toml ~/.salita/config.toml
# Edit config.toml to set your directories

# Start the daemon
salita serve

# Or use as an MCP server for Claude Code
salita mcp
```

## Claude Code Integration

Add to your Claude Code MCP config:

```json
{
  "mcpServers": {
    "salita": {
      "type": "stdio",
      "command": "salita",
      "args": ["mcp"]
    }
  }
}
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `list_devices` | All mesh devices + online/offline status |
| `list_files` | List files in a directory on any device |
| `search_files` | Glob search across devices |
| `read_file` | Read file content from any device |
| `file_info` | File metadata (size, type, modified) |

## Configuration

```toml
[server]
host = "0.0.0.0"
port = 6969

[[directories]]
label = "documents"
path = "~/Documents"

[[directories]]
label = "projects"
path = "~/Projects"
```

Files are addressed by `(device, directory_label, relative_path)` — absolute paths never cross the wire.

## Tech Stack

- **Rust** + **Axum** — HTTP server
- **rmcp** — MCP SDK (stdio transport)
- **SQLite** — device registry (WAL mode, r2d2 pool)
- **mdns-sd** — zero-config peer discovery
- **reqwest** — peer HTTP client

## Development

```bash
cargo build
cargo test
cargo run -- serve
cargo run -- mcp
```

## License

[MIT](LICENSE)

## Repository

https://github.com/Dorky-Robot/salita
