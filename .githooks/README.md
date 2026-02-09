# Git Hooks for Salita

## Installation

To enable these hooks for your local repository:

```bash
git config core.hooksPath .githooks
```

## Hooks

### pre-commit
Runs before each commit:
- ✅ Code formatting (`cargo fmt`)
- ✅ Linting (`cargo clippy`)
- ✅ Unit tests (`cargo test --lib`)

### pre-push  
Runs before pushing to remote:
- ✅ Release build (`cargo build --release`)
- ✅ All tests (`cargo test`)
- ✅ E2E tests (if server is running)

## Running E2E Tests Manually

Start the server with test seed enabled:
```bash
SALITA_TEST_SEED=1 cargo run
```

In another terminal, run E2E tests:
```bash
cargo test --test e2e_dashboard -- --ignored
```

## Skipping Hooks

If you need to skip hooks temporarily:
```bash
git commit --no-verify
git push --no-verify
```

Use sparingly - hooks are there to catch issues early!
