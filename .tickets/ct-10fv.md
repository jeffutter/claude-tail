---
id: ct-10fv
status: open
deps: [ct-7hj2, ct-jsem]
links: []
created: 2026-02-07T02:09:17Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Use nextest for tests

Update tests to run with nextest https://github.com/nextest-rs/nextest

## Design

### Overview

Migrate the test infrastructure from `cargo test` to `cargo nextest` for faster parallel test execution and better test output. This is a straightforward task that requires:
1. Creating a nextest configuration file
2. Updating the CI workflow

### Prerequisites

This ticket depends on ct-jsem and ct-7hj2 which add the actual tests. Nextest is a test runner, not a test generator - tests must exist before the runner can be configured.

### Implementation Steps

#### Step 1: Create nextest configuration

Create `.config/nextest.toml`:

```toml
[profile.default]
# Stop on first failure for faster local feedback
fail-fast = true
# Default timeout per test (60 seconds)
slow-timeout = { period = "60s" }

[profile.ci]
# CI profile - run all tests even if some fail
fail-fast = false
# Stricter timeout in CI
slow-timeout = { period = "30s" }
```

#### Step 2: Update CI workflow

Modify `.github/workflows/ci.yml` test job:

```yaml
test:
  name: Test Suite
  runs-on: ubuntu-latest
  steps:
    - name: Checkout repository
      uses: actions/checkout@v6
    - name: Install Rust toolchain
      uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - name: Install cargo-nextest
      uses: taiki-e/install-action@nextest
    - name: Run tests
      run: cargo nextest run --all-features --workspace --profile ci
```

### Notes

- **No doctests concern**: The codebase has no doctests, so nextest's doctest limitation is not an issue
- **No benchmark impact**: Nextest is for unit tests only; Divan benchmarks (from ct-ad3b) run separately via `cargo bench`
- **Installation method**: Using `taiki-e/install-action@nextest` is the recommended approach for GitHub Actions - it's faster than `cargo install` and handles caching

### Acceptance Criteria

- [ ] `.config/nextest.toml` created with default and ci profiles
- [ ] `.github/workflows/ci.yml` updated to use nextest
- [ ] All existing tests pass with `cargo nextest run --all-features --workspace`
- [ ] CI workflow passes with nextest

