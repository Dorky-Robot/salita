#!/usr/bin/env bash
set -euo pipefail

DATA_DIR="${SALITA_DATA_DIR:-$HOME/.salita}"
INSTALL_DIR="/usr/local/bin"
CA_LABEL="Salita Local CA"

echo "==> Salita Uninstaller"
echo ""

# Remove binary
if [ -f "$INSTALL_DIR/salita" ]; then
    echo "==> Removing $INSTALL_DIR/salita..."
    sudo rm -f "$INSTALL_DIR/salita"
    echo "    Removed."
else
    echo "==> No binary found at $INSTALL_DIR/salita"
fi

# Remove CA from macOS keychains
if [ "$(uname)" = "Darwin" ]; then
    echo "==> Removing \"$CA_LABEL\" from keychains..."

    security delete-certificate -c "$CA_LABEL" "$HOME/Library/Keychains/login.keychain-db" 2>/dev/null && {
        echo "    Removed \"$CA_LABEL\" from login keychain."
    } || {
        echo "    \"$CA_LABEL\" not found in login keychain (already removed)."
    }

    sudo security delete-certificate -c "$CA_LABEL" /Library/Keychains/System.keychain 2>/dev/null && {
        echo "    Removed \"$CA_LABEL\" from system keychain."
    } || {
        echo "    \"$CA_LABEL\" not found in system keychain (already removed)."
    }
fi

# Remove data directory
if [ -d "$DATA_DIR" ]; then
    echo ""
    read -rp "==> Delete all Salita data at $DATA_DIR? (y/N) " confirm
    if [[ "$confirm" =~ ^[Yy]$ ]]; then
        rm -rf "$DATA_DIR"
        echo "    Deleted $DATA_DIR"
    else
        echo "    Kept $DATA_DIR"
    fi
else
    echo "==> No data directory found at $DATA_DIR"
fi

echo ""
echo "==> Uninstall complete."
echo ""
