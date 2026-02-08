.PHONY: dev build run clean

# Development mode with live reload
dev:
	@echo "🔧 Starting development server with live reload..."
	@echo "📝 Watching: src/ templates/ Cargo.toml"
	@echo "Press Ctrl+C to stop"
	@echo ""
	@pkill -9 -f "target/debug/salita" 2>/dev/null || true
	@find src templates Cargo.toml -type f | entr -rn cargo run

# Build release version
build:
	cargo build --release

# Run release version (no reload)
run:
	cargo run --release

# Clean build artifacts
clean:
	cargo clean

# Quick check (compile without running)
check:
	cargo check
