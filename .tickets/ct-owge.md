---
id: ct-owge
status: open
deps: []
links: []
created: 2026-02-14T23:38:32Z
type: chore
priority: 3
assignee: Jeffery Utter
tags: [planned]
---
# Add nextest support for faster parallel test execution

Replace cargo test with cargo nextest as the test runner for faster parallel execution and better output.

## Changes needed
1. .config/nextest.toml — config file with two profiles:
   - default: fail-fast = true (stop on first failure for fast local feedback), slow-timeout = 60s
   - ci: fail-fast = false (run all tests), slow-timeout = 30s

2. .github/workflows/ci.yml — update test step to install and use nextest:
   - Add step: uses: taiki-e/install-action@nextest
   - Change run: cargo test --all-features --workspace
     to:   cargo nextest run --all-features --workspace --profile ci

3. flake.nix — add cargo-nextest to devShell packages

## Design

No sub-tickets needed — all three changes form a single coherent changeset that ships together.

### Verified current state (2026-02-14)
- CI uses `cargo test --all-features --workspace` (ci.yml:20)
- flake.nix devShell has: vhs, rustToolchain, pkg-config, lefthook (flake.nix:35-40)
- lefthook pre-commit runs `cargo test` (lefthook.yml:22)
- No `.config/` directory exists yet
- Single-package project (no workspace)

### Step 1: Create `.config/nextest.toml`

Create the directory and config file with two profiles:

```toml
[profile.default]
fail-fast = true
slow-timeout = { period = "60s" }

[profile.ci]
fail-fast = false
slow-timeout = { period = "30s" }
```

- `default` profile: fail-fast for quick local feedback, generous 60s slow-timeout for property tests
- `ci` profile: run all tests even if some fail, 30s slow-timeout to flag unexpectedly slow tests

### Step 2: Update `.github/workflows/ci.yml`

Replace the test job steps (ci.yml:13-20) to install and use nextest:

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
      - name: Install nextest
        uses: taiki-e/install-action@nextest
      - name: Run tests
        run: cargo nextest run --all-features --workspace --profile ci
```

Changes:
- Add `taiki-e/install-action@nextest` step before test run
- Replace `cargo test` with `cargo nextest run --profile ci`

**Note:** `cargo nextest` does not support doc tests. If doc tests are needed in CI, add a separate `cargo test --doc` step. Currently there are no doc tests in this project, so this is not needed.

### Step 3: Update `flake.nix`

Add `cargo-nextest` to devShell buildInputs (flake.nix:35-40):

```nix
buildInputs = with pkgs; [
  vhs
  rustToolchain
  cargo-nextest
  pkg-config
  lefthook
];
```

### Step 4 (optional): Update `lefthook.yml`

Update the pre-commit test command (lefthook.yml:22) to use nextest:

```yaml
    - name: Test
      glob: "**/*.rs"
      run: cargo nextest run --lib
```

This gives faster parallel test execution locally. The `--lib` flag keeps pre-commit fast by skipping integration and property tests (consistent with MEMORY.md note that lefthook runs `cargo test --lib` only).

**Note:** If `cargo-nextest` may not be installed on all contributor machines, keep `cargo test` as fallback. Since `flake.nix` ensures it's available in the dev shell, this should be safe.

### Verification

1. `cargo nextest run` — all unit tests pass locally
2. `cargo nextest run --profile ci` — CI profile works
3. Push to a branch and verify CI passes with nextest
4. Confirm lefthook pre-commit still works

### Relationship to ct-xwz9

ct-xwz9 adds `Bash(cargo nextest:*)` to Claude settings permissions. Independent work — can be done before or after this ticket. Neither blocks the other.

## Notes
Source commit: e1fae5a on ai-slop-refactor. Independent of other tickets. Verified against current main branch state.

