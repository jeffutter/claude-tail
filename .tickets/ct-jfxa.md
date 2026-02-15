---
id: ct-jfxa
status: closed
deps: [ct-kkil]
links: []
created: 2026-02-14T23:38:24Z
type: task
priority: 3
assignee: Jeffery Utter
tags: [planned]
---
# Add divan benchmarks for JSONL parsing performance

Add a divan benchmark suite for the JSONL parser to establish performance baselines and enable regression detection.

## Benchmark categories (from ai-slop-refactor)
1. Full parse: 100, 1000, 10000 entries — establishes linear scaling baseline (~0.85µs/entry)
2. Incremental: resume from various file positions (start, 25%, 50%, 75%, end)
3. Error recovery: 0%, 1%, 5%, 10% synthetic error rates
4. Tool merge: merge tool results into tool calls, 100–1000 entries

## Observed baselines (from ai-slop-refactor run on reference hardware)
- 100 entries: ~94µs
- 1000 entries: ~833µs
- 10000 entries: ~8.4ms
- Incremental from 50%: ~1.0ms
- Error handling overhead: ~3% at 10% error rate

## Setup required
- Add benches/parser.rs
- Add divan to dev-dependencies in Cargo.toml
- Add [[bench]] section pointing to benches/parser.rs with harness = false
- Add src/lib.rs exposing pub mod logs so benchmarks can access parser internals

Example bench structure:
  #[divan::bench(args = [100, 1000, 10000])]
  fn full_parse(b: divan::Bencher, n: usize) {
      let content = generate_jsonl_content(n);
      b.bench(|| parse_jsonl_content(&content))
  }

## Files
- benches/parser.rs (new)
- src/lib.rs (new, exposes pub mod logs)
- Cargo.toml

## Design

No sub-tickets needed — this is a focused single-session task across 3 files.

### Step 1: Create src/lib.rs

Expose the `logs` module so the benchmark crate can import parser functions:

```rust
pub mod logs;
```

Only `logs` is needed. The `app`, `input`, `themes`, and `ui` modules remain private to the binary. The existing `mod logs;` in `main.rs` stays — Cargo supports both a `[[bin]]` and a `[lib]` target in the same crate, each with their own module tree.

Note: After ct-kkil lands, LogEntry will be a tagged enum. The benchmarks don't construct LogEntry directly — they generate raw JSONL strings and feed them through the parser.

### Step 2: Update Cargo.toml

Add dev-dependency and bench target:

```toml
[dev-dependencies]
divan = "0.1"
tempfile = "3"

[[bench]]
name = "parser"
harness = false
```

`tempfile` is needed because `parse_jsonl_file` and `parse_jsonl_from_position` take `&Path` — we write generated JSONL to a temp file, then parse it. This keeps benchmarks exercising the real file I/O path (which is what production uses).

### Step 3: Create benches/parser.rs

Four benchmark groups with a shared data generator:

#### Data generator: `generate_jsonl_file(n: usize) -> NamedTempFile`

Writes `n` JSONL lines to a temp file. Each line is a valid JSON object matching the LogEntry format. Mix of entry types:
- ~40% user messages (short text)
- ~40% assistant messages (with ContentBlock::Text)
- ~20% assistant messages with ToolUse blocks (for tool merge benchmarks)

Use deterministic content (no random) so benchmarks are reproducible. A simple rotating pattern over the entry types suffices.

For error-rate benchmarks, a variant `generate_jsonl_file_with_errors(n: usize, error_rate: f64) -> NamedTempFile` injects malformed JSON lines at the specified rate (e.g., truncated lines, invalid JSON).

#### Group 1: Full parse — `full_parse`

```rust
#[divan::bench(args = [100, 1000, 10000])]
fn full_parse(bencher: divan::Bencher, n: usize) {
    let file = generate_jsonl_file(n);
    bencher.bench(|| {
        parse_jsonl_file(file.path()).unwrap()
    });
}
```

Measures end-to-end parsing of complete files at different sizes. Establishes linear scaling baseline.

#### Group 2: Incremental parse — `incremental_parse`

```rust
#[divan::bench(args = [0, 25, 50, 75, 100])]
fn incremental_parse(bencher: divan::Bencher, pct: usize) {
    let file = generate_jsonl_file(1000);
    let total_bytes = std::fs::metadata(file.path()).unwrap().len();
    // Find the nearest newline boundary at pct% of file
    let position = find_line_boundary(file.path(), total_bytes * pct as u64 / 100);
    bencher.bench(|| {
        parse_jsonl_from_position(file.path(), position).unwrap()
    });
}
```

Needs a helper `find_line_boundary(path, approx_pos) -> u64` that seeks to the nearest newline at or after `approx_pos`. This ensures we resume at a valid line boundary.

The 100% case (position at EOF) verifies that resuming past the end is fast (should be ~0).

#### Group 3: Error recovery — `error_recovery`

```rust
#[divan::bench(args = [0.0, 0.01, 0.05, 0.10])]
fn error_recovery(bencher: divan::Bencher, error_rate: f64) {
    let file = generate_jsonl_file_with_errors(1000, error_rate);
    bencher.bench(|| {
        parse_jsonl_file(file.path()).unwrap()
    });
}
```

Measures parsing overhead when malformed lines are present. The 0% case serves as baseline.

Note: divan's `args` requires a type that implements `ToString`. For `f64`, this works. If it doesn't, use integer percentages (0, 1, 5, 10) and divide by 100.0 inside the function.

#### Group 4: Tool merge — `tool_merge`

```rust
#[divan::bench(args = [100, 500, 1000])]
fn tool_merge(bencher: divan::Bencher, n: usize) {
    let file = generate_jsonl_file_with_tools(n);
    let parsed = parse_jsonl_file(file.path()).unwrap();
    bencher.bench_local(|| {
        merge_tool_results(parsed.entries.clone())
    });
}
```

Needs `generate_jsonl_file_with_tools(n)` that generates `n` entries where ~50% are ToolUse blocks followed by matching ToolResult entries. Benchmarks `merge_tool_results` in isolation (parsing done in setup).

Uses `bench_local` because `Vec<DisplayEntry>` clone happens in the benchmark iteration. Alternatively, use `bench` with `divan::black_box` on the clone.

#### Main function

```rust
fn main() {
    divan::main();
}
```

### Step 4: Verify

1. `cargo check` — confirms lib.rs + bench target compile
2. `cargo bench` — runs all benchmarks, produces baseline numbers
3. Verify no regressions in `cargo test --lib` (lib.rs shouldn't break existing tests)
4. Verify `cargo clippy` is clean

### Scope boundaries

- **3 files change**: `src/lib.rs` (new), `benches/parser.rs` (new), `Cargo.toml` (modified)
- No changes to existing source files beyond Cargo.toml
- Benchmarks are read-only consumers of the parser API
- If ct-5vmd (StreamDeserializer) lands before this ticket is executed, the benchmarks still work — they call the same public API (`parse_jsonl_file`, `parse_jsonl_from_position`, `merge_tool_results`)

### Execution note

ct-xwz9 (cargo bench permissions in .claude/settings.json) should ideally be completed before executing this ticket so the agent can run `cargo bench`. Not a hard dependency — the code can be written without it.

## Notes
Source commit: 8d14202 on ai-slop-refactor. The StreamDeserializer refactor (ct-5vmd) later removed some duplicate stream-specific bench variants, so the final bench file will be the cleaned-up version. Depends on ct-kkil (needs lib.rs + logs module to be accessible from bench context).

