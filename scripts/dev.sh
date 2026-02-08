#!/bin/bash
# Development server with live reloading

set -e

echo "🔧 Starting Salita in development mode with live reloading..."
echo "📝 Watching for changes in src/ and templates/"
echo ""

# Kill any existing salita processes
pkill -9 -f "target/debug/salita" 2>/dev/null || true
pkill -9 -f "target/release/salita" 2>/dev/null || true

# Function to start the server
start_server() {
    echo "🚀 Starting server..."
    cargo build 2>&1 | grep -E "(Compiling|Finished|error)" || true
    cargo run &
    SERVER_PID=$!
    echo "✓ Server running (PID: $SERVER_PID)"
    echo ""
}

# Function to restart the server
restart_server() {
    echo ""
    echo "🔄 Changes detected, restarting..."
    pkill -9 -f "target/debug/salita" 2>/dev/null || true
    sleep 1
    start_server
}

# Start initial server
start_server

# Watch for changes and restart
find src templates -type f \( -name "*.rs" -o -name "*.html" -o -name "*.toml" \) | entr -r sh -c 'pkill -9 -f "target/debug/salita" 2>/dev/null || true; sleep 1; cargo run'
