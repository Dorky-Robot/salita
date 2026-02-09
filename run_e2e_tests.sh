#!/bin/bash
# E2E Test Runner for Salita

set -e

echo "🧪 Salita E2E Test Runner"
echo "=========================="

# Check if server is running
if ! curl -s http://localhost:6969/health > /dev/null 2>&1; then
    echo "❌ Server not running at localhost:6969"
    echo ""
    echo "Start the server first:"
    echo "  SALITA_TEST_SEED=1 cargo run"
    exit 1
fi

echo "✅ Server is running"
echo ""

# Run E2E tests
echo "Running E2E tests..."
SALITA_TEST_SEED=1 cargo test --test e2e_dashboard -- --ignored --test-threads=1

echo ""
echo "✅ All E2E tests passed!"
