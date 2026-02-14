---
id: ct-jfxa
status: open
deps: [ct-kkil]
links: []
created: 2026-02-14T23:38:24Z
type: task
priority: 3
assignee: Jeffery Utter
tags: [needs-plan]
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

## Notes
Source commit: 8d14202 on ai-slop-refactor. The StreamDeserializer refactor (ct-5vmd) later removed some duplicate stream-specific bench variants, so the final bench file will be the cleaned-up version. Depends on ct-kkil (needs lib.rs + logs module to be accessible from bench context). Re-planning required.

