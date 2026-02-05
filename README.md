# Salita

A personal home server. Your own corner of the internet.

Salita is a self-hosted web platform for micro-blogging, media sharing, and social features — all under your control. No passwords, no third-party auth providers. Just passkeys.

## Features

- **Passkey authentication** — passwordless login via Touch ID, Face ID, or security keys (WebAuthn)
- **Micro-blogging** — posts with images, reactions, and comments
- **Single binary** — one Rust binary, SQLite database, zero external services
- **Self-contained** — all data lives in `~/.salita/` by default

## Quick Start

```bash
cargo build --release
./target/release/salita
```

Open `http://localhost:6969` and create your admin account with a passkey.

## Configuration

Copy `config.example.toml` to `~/.salita/config.toml` to customize, or use CLI flags:

```bash
salita --port 8080 --data-dir /path/to/data
```

## Tech Stack

- **Rust** + **Axum** — web framework
- **SQLite** — database (WAL mode, r2d2 pool)
- **Askama** — templates
- **WebAuthn-rs** — passkey authentication
- **HTMX** + **Tailwind CSS** — frontend

## Development

```bash
# Run in development
cargo run

# Run tests
cargo test

# Run E2E tests (headless)
npx playwright test --project=chromium

# Run passkey E2E tests (requires Touch ID)
npx playwright test --project=passkey --headed
```

## License

[MIT](LICENSE)
