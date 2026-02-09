#!/bin/bash
# Claude Code lifecycle hooks for Salita
# These hooks run automatically during Claude Code operations

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo_info() {
    echo -e "${GREEN}[Hook]${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}[Hook]${NC} $1"
}

echo_error() {
    echo -e "${RED}[Hook]${NC} $1"
}

# Pre-edit hook: Run before Claude edits files
pre_edit() {
    echo_info "Running pre-edit checks..."

    # Check if Cargo.toml has been modified
    if git diff --cached --name-only | grep -q "Cargo.toml"; then
        echo_warn "Cargo.toml changed - will need cargo check"
    fi
}

# Post-edit hook: Run after Claude edits files
post_edit() {
    echo_info "Running post-edit checks..."

    # If Rust files were changed, run cargo check
    if git diff --name-only | grep -q "\.rs$"; then
        echo_info "Rust files changed - running cargo check..."
        if cargo check 2>&1 | tail -20; then
            echo_info "✓ Cargo check passed"
        else
            echo_error "✗ Cargo check failed"
            return 1
        fi
    fi

    # If templates were changed, remind to rebuild
    if git diff --name-only | grep -q "templates/"; then
        echo_warn "Templates changed - remember to rebuild (cargo build)"
    fi
}

# Pre-commit hook: Run before commits
pre_commit() {
    echo_info "Running pre-commit checks..."

    # Run cargo fmt check
    if ! cargo fmt -- --check; then
        echo_warn "Code not formatted - running cargo fmt..."
        cargo fmt
        echo_info "✓ Code formatted"
    fi

    # Run cargo clippy
    echo_info "Running clippy..."
    if cargo clippy -- -D warnings 2>&1 | tail -20; then
        echo_info "✓ Clippy passed"
    else
        echo_warn "Clippy found issues (not blocking)"
    fi

    # Run tests
    echo_info "Running tests..."
    if cargo test 2>&1 | tail -20; then
        echo_info "✓ Tests passed"
    else
        echo_error "✗ Tests failed"
        return 1
    fi
}

# Pre-push hook: Run before pushing to remote
pre_push() {
    echo_info "Running pre-push checks..."

    # Build in release mode
    echo_info "Building release binary..."
    if cargo build --release 2>&1 | tail -20; then
        echo_info "✓ Release build succeeded"
    else
        echo_error "✗ Release build failed"
        return 1
    fi

    # Run E2E tests if server is running
    if curl -s http://localhost:6969/health > /dev/null 2>&1; then
        echo_info "Running E2E tests..."
        if SALITA_TEST_SEED=1 cargo test --test e2e_dashboard -- --ignored 2>&1 | tail -30; then
            echo_info "✓ E2E tests passed"
        else
            echo_warn "E2E tests failed (not blocking)"
        fi
    else
        echo_warn "Server not running - skipping E2E tests"
    fi
}

# Main hook dispatcher
case "${1}" in
    pre-edit)
        pre_edit
        ;;
    post-edit)
        post_edit
        ;;
    pre-commit)
        pre_commit
        ;;
    pre-push)
        pre_push
        ;;
    *)
        echo_error "Unknown hook: ${1}"
        exit 1
        ;;
esac
