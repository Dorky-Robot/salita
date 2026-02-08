#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL_DIR="/usr/local/bin"

echo "==> Salita Installer"
echo ""

# Check for Rust toolchain
if ! command -v cargo &>/dev/null; then
    echo "Error: cargo not found. Install Rust first: https://rustup.rs"
    exit 1
fi

# Build release binary
echo "==> Building Salita..."
cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"

# Install to PATH
echo "==> Installing to $INSTALL_DIR/salita..."
sudo cp "$SCRIPT_DIR/target/release/salita" "$INSTALL_DIR/salita"

echo ""
echo "==> Installation complete!"
echo ""
echo "    Run:  salita"
echo ""
echo "    On first launch, Salita will:"
echo "      - Create data directory at ~/.salita/"
echo "      - Generate TLS certificates"
echo "      - Trust the CA in your macOS login keychain"
echo "      - Start HTTPS on https://localhost:6969"
echo ""
echo "    To uninstall:  ./scripts/uninstall.sh"
echo ""
