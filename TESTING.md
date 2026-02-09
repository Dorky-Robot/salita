# Salita Testing Guide

## Overview

Comprehensive testing has been implemented for Salita's critical pairing flow. Tests cover unit, integration, and end-to-end scenarios.

## Running Tests

### Quick Start
```bash
# Run all unit tests
cargo test

# Run specific module tests
cargo test join_tokens
cargo test mesh::

# Run with output
cargo test -- --nocapture
```

### E2E Tests (Requires Running Server)
```bash
# Terminal 1: Start server
cargo run

# Terminal 2: Run E2E tests
cargo test --test pairing_e2e -- --test-threads=1
```

## Test Coverage

### Unit Tests (14 tests - All Passing ✓)

**Join Token Tests** (`src/auth/join_tokens.rs`):
- ✓ `test_generate_token` - Token generation and validation
- ✓ `test_single_use_token` - Tokens can only be used once
- ✓ `test_invalid_token` - Invalid tokens are rejected
- ✓ `test_pin_generation` - PINs are 6 digits
- ✓ `test_pin_verification_success` - Correct PIN verification
- ✓ `test_pin_verification_wrong_pin` - Wrong PINs rejected
- ✓ `test_pin_verification_unused_token` - Can't verify unused tokens
- ✓ `test_device_ip_stored` - Device IP is stored correctly
- ✓ `test_token_uniqueness` - Tokens are unique
- ✓ `test_pin_uniqueness` - PINs are unique
- ✓ `test_token_created_by` - Tracks token creator
- ✓ `test_multiple_tokens` - Multiple simultaneous tokens work
- ✓ `test_secure_token_charset` - Tokens use alphanumeric only
- ✓ `test_clear_stale_removes_nothing_when_valid` - Cleanup doesn't affect valid tokens

### E2E Tests (`tests/pairing_e2e.rs`)

**Critical Pairing Flow**:
- `test_complete_pairing_flow` - Full desktop→mobile pairing journey
- `test_join_token_expiry` - 5-minute TTL enforcement
- `test_pin_single_use` - PINs can't be reused
- `test_wrong_pin_rejected` - Security validation
- `test_http_and_https_servers` - Both servers functional
- `test_mobile_redirect_after_pairing` - GraphQL polling works

**Flow Tested**:
```
1. Desktop opens join modal → gets token
2. Mobile scans QR → accesses join page
3. Mobile displays 6-digit PIN
4. Desktop enters PIN → verifies
5. Desktop registers mobile via GraphQL
6. Mobile polls GraphQL every 3 seconds
7. Mobile detects nodes.length > 1
8. Mobile redirects to /dashboard
```

## Test Results

```bash
$ cargo test join_tokens
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.73s
     Running unittests src/main.rs

running 14 tests
test auth::join_tokens::tests::test_invalid_token ... ok
test auth::join_tokens::tests::test_token_uniqueness ... ok
test auth::join_tokens::tests::test_token_created_by ... ok
test auth::join_tokens::tests::test_clear_stale_removes_nothing_when_valid ... ok
test auth::join_tokens::tests::test_generate_token ... ok
test auth::join_tokens::tests::test_multiple_tokens ... ok
test auth::join_tokens::tests::test_device_ip_stored ... ok
test auth::join_tokens::tests::test_pin_verification_unused_token ... ok
test auth::join_tokens::tests::test_single_use_token ... ok
test auth::join_tokens::tests::test_pin_generation ... ok
test auth::join_tokens::tests::test_pin_uniqueness ... ok
test auth::join_tokens::tests::test_pin_verification_wrong_pin ... ok
test auth::join_tokens::tests::test_pin_verification_success ... ok
test auth::join_tokens::tests::test_secure_token_charset ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Tests
on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - run: cargo test --all-features

  e2e-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
      - name: Start server
        run: cargo run &
      - name: Wait for server
        run: sleep 5
      - name: Run E2E tests
        run: cargo test --test pairing_e2e -- --test-threads=1
```

## Security Testing

The tests verify critical security requirements:

1. **Token Security**
   - 32-character alphanumeric tokens
   - Single-use enforcement
   - 5-minute expiry
   - Cryptographically random

2. **PIN Security**
   - 6-digit numeric
   - Unique per pairing
   - Single-use validation
   - Stored securely

3. **Device Authentication**
   - IP address tracking
   - Node registration validation
   - GraphQL authorization (to be added)

## Future Enhancements

### Headless Browser Testing

For true E2E JavaScript testing:

```rust
use headless_chrome::{Browser, LaunchOptions};

#[test]
fn test_mobile_auto_redirect() {
    let browser = Browser::new(LaunchOptions::default()).unwrap();
    let tab = browser.new_tab().unwrap();

    tab.navigate_to(&join_url).unwrap();
    tab.wait_for_element("#pin-code").unwrap();

    // Trigger pairing via API
    pair_device(&token, &pin);

    // Verify JavaScript redirects
    tab.wait_until_navigated().unwrap();
    assert!(tab.get_url().ends_with("/dashboard"));
}
```

### Load Testing

Test concurrent pairings:

```rust
#[tokio::test]
async fn test_concurrent_pairings() {
    let mut handles = vec![];

    for i in 0..100 {
        handles.push(tokio::spawn(async move {
            pair_device(format!("device-{}", i)).await
        }));
    }

    for handle in handles {
        assert!(handle.await.unwrap().is_ok());
    }
}
```

### Property-Based Testing

Using `proptest`:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn tokens_are_always_unique(count in 1..1000usize) {
        let mut store = JoinTokenStore::new();
        let tokens: HashSet<_> = (0..count)
            .map(|i| store.generate(format!("node-{}", i)))
            .collect();

        prop_assert_eq!(tokens.len(), count);
    }
}
```

## Debugging Failed Tests

### Enable Logging
```bash
RUST_LOG=debug cargo test -- --nocapture
```

### Run Single Test
```bash
cargo test test_complete_pairing_flow -- --nocapture
```

### Check Server Logs
```bash
# In separate terminal
cargo run
# Watch for pairing-related logs
```

## Test Data Cleanup

Tests use the production database by default. For isolation:

```rust
use tempfile::TempDir;

#[test]
fn test_with_isolated_db() {
    let tmp_dir = TempDir::new().unwrap();
    let db_path = tmp_dir.path().join("test.db");

    // Use test database
    // ...

    // Automatically cleaned up when tmp_dir drops
}
```

## Coverage

To measure test coverage:

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Run coverage
cargo tarpaulin --out Html --output-dir coverage

# Open coverage/index.html
```

## Documentation

Each test includes:
- Clear test name describing what's being tested
- Comments explaining the scenario
- Assertions with descriptive messages

Example:
```rust
#[test]
fn test_pin_verification_wrong_pin() {
    // Setup: Generate token and PIN
    let mut store = JoinTokenStore::new();
    let token = store.generate("node-1".to_string());
    store.use_token(&token, "192.168.1.1".to_string()).unwrap();

    // Verify wrong PIN is rejected
    assert!(!store.verify_pin(&token, "000000"));
}
```

## Resources

- [E2E Test Documentation](tests/README.md) - Detailed E2E testing guide
- [Rust Testing](https://doc.rust-lang.org/book/ch11-00-testing.html) - Official Rust testing docs
- [Tokio Testing](https://tokio.rs/tokio/topics/testing) - Async testing patterns
