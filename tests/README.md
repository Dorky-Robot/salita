# Salita E2E Tests

## Overview

This directory contains end-to-end tests for critical Salita flows, particularly the device pairing flow which is fundamental to the mesh network concept.

## Running Tests

### All Tests
```bash
cargo test
```

### E2E Tests Only
```bash
cargo test --test pairing_e2e
```

### With Server Running
The E2E tests in `pairing_e2e.rs` require a running Salita server. Start the server first:

```bash
# Terminal 1: Start server
cargo run

# Terminal 2: Run E2E tests
cargo test --test pairing_e2e -- --test-threads=1
```

**Note:** Use `--test-threads=1` to run tests sequentially since they share the same server instance.

## Test Coverage

### Critical Pairing Flow (`test_complete_pairing_flow`)

Tests the complete user journey:

1. **Desktop**: Opens join modal, generates join token
2. **Mobile**: Scans QR code, accesses `/join` page
3. **Mobile**: Sees 6-digit PIN on screen
4. **Desktop**: Enters PIN, verifies it
5. **Desktop**: Registers mobile device via GraphQL
6. **Mobile**: Polls GraphQL every 3 seconds
7. **Mobile**: Detects it's been added to mesh (nodes.length > 1)
8. **Mobile**: Redirects to `/dashboard`

This test verifies the entire flow works end-to-end without any manual intervention.

### Security Tests

- `test_join_token_expiry`: Tokens expire after 5 minutes
- `test_pin_single_use`: PINs can only be used once
- `test_wrong_pin_rejected`: Wrong PINs are rejected

### Infrastructure Tests

- `test_http_and_https_servers`: Both HTTP (6968) and HTTPS (6969) servers work
- `test_mobile_redirect_after_pairing`: Mobile's GraphQL polling and redirect logic works

## Test Architecture

### HTTP vs HTTPS

The tests use the HTTP server (port 6968) by default because:
- Mobile devices don't need to trust HTTPS certificates first
- Simpler test setup (no cert management)
- Same functionality is exposed on both servers

### Client Configuration

```rust
let client = Client::builder()
    .danger_accept_invalid_certs(true)  // For HTTPS tests
    .redirect(reqwest::redirect::Policy::none())  // Don't auto-follow redirects
    .build()
    .unwrap();
```

### HTML Parsing

Tests parse HTML responses to extract:
- Join URLs from modal
- Tokens from URLs
- PINs from join page

Helper functions (`extract_join_url`, `extract_pin_from_html`, etc.) handle this parsing.

## Future Improvements

### Headless Browser Tests

For true E2E testing of the redirect flow, consider using:

```toml
[dev-dependencies]
headless_chrome = "1"
```

Example:
```rust
use headless_chrome::{Browser, LaunchOptions};

#[test]
fn test_mobile_redirect_with_browser() {
    let browser = Browser::new(LaunchOptions::default()).unwrap();
    let tab = browser.new_tab().unwrap();

    // Navigate to join page
    tab.navigate_to(&join_url).unwrap();

    // Wait for PIN to appear
    tab.wait_for_element("div.pin-display__code").unwrap();

    // Simulate desktop entering PIN (via API call)
    // ...

    // Wait for redirect
    tab.wait_until_navigated().unwrap();

    assert!(tab.get_url().ends_with("/dashboard"));
}
```

### Database Isolation

For parallel test execution:
- Use separate SQLite databases per test
- Clean up after each test
- Use transactions that rollback

### Mock Time

For token expiry tests, mock the clock:

```rust
use mock_instant::MockClock;

MockClock::advance(Duration::from_secs(301));
assert!(token.is_expired());
```

## CI/CD Integration

### GitHub Actions

```yaml
name: E2E Tests
on: [push, pull_request]

jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable

      - name: Start Salita Server
        run: cargo run &

      - name: Wait for Server
        run: sleep 5

      - name: Run E2E Tests
        run: cargo test --test pairing_e2e -- --test-threads=1
```

## Troubleshooting

### Server Not Running

```
Error: Connection refused (os error 61)
```

**Fix**: Start the server with `cargo run` in another terminal.

### Port Already in Use

```
Error: Address already in use (os error 48)
```

**Fix**: Kill existing Salita processes:
```bash
pkill -f salita
```

### Test Timeouts

If tests timeout, increase the polling attempts:
```rust
let max_attempts = 20;  // Increase from 10
```

### Certificate Errors (HTTPS tests)

```
Error: Certificate verify failed
```

**Fix**: Use the HTTP server (port 6968) for tests, or configure the client to accept invalid certs:
```rust
.danger_accept_invalid_certs(true)
```
