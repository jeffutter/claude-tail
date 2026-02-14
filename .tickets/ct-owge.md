---
id: ct-owge
status: open
deps: []
links: []
created: 2026-02-14T23:38:32Z
type: chore
priority: 3
assignee: Jeffery Utter
tags: [needs-plan]
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

## Notes
Source commit: e1fae5a on ai-slop-refactor. Independent of other tickets. Re-planning required to verify CI workflow and flake structure still match current state.

