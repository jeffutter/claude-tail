---
id: ct-6qk4
status: closed
deps: [ct-jsem, ct-7hj2, ct-ad3b]
links: []
created: 2026-02-07T01:55:09Z
type: task
priority: 3
assignee: Jeffery Utter
tags: [planned, research]
---
# Investigate StreamDeserializer for JSONL parsing

Research whether serde_json::StreamDeserializer can replace the current line-based parsing while maintaining support for incremental parsing and incomplete line handling.

**Scope**: Research and analysis only - DO NOT modify the main codebase. You may create an alternative proof-of-concept implementation in a separate module or branch if helpful for comparison.

**Must verify**:
- Byte position tracking via byte_offset()
- Error recovery behavior
- Incomplete JSON at EOF handling (critical for streaming logs)
- Memory efficiency compared to current approach
- Performance characteristics

**Deliverable**: Detailed written analysis documenting trade-offs, test results from POC (if created), and clear recommendation on whether to proceed with refactoring.

## Design

### Prerequisites

This research depends on three tickets that must complete first:
- **ct-jsem**: Unit tests for LogEntry parsing (validates deserialization layer)
- **ct-7hj2**: Tests for incremental JSONL parsing (validates byte position tracking, incomplete line handling)
- **ct-ad3b**: Benchmarks for parsing performance (establishes baseline metrics)

These provide the test harness and baselines needed for rigorous comparison.

### Research Questions

| Question | Method | Success Criteria |
|----------|--------|------------------|
| Does `byte_offset()` track position accurately? | POC tests with known file content | Offset matches expected byte counts for UTF-8 content |
| Can it distinguish incomplete vs malformed JSON? | Test `error.is_eof()` vs `error.is_syntax()` | Correct classification for both cases |
| Does it handle incomplete lines at EOF correctly? | Simulate streaming writes | Position not advanced for incomplete lines |
| What's the performance delta? | Run ct-ad3b benchmarks on both parsers | Document % difference at 100/1000/10000 entries |
| Is the re-parsing overhead acceptable? | Benchmark incremental parsing | <20% overhead for typical 1-10MB session files |

### Current Parser Analysis

**Location**: `src/logs/parser.rs`

**Approach**:
- Reads entire file into `String` via `read_to_string()`
- Iterates with `.lines()` (zero-copy string slices)
- Manual byte position tracking: `bytes_consumed + line.len() + 1` (for newline)
- Incomplete line detection: checks if `with_newline <= content.len()`
- Resumption: `Seek::Start(position)` for O(1) file access

**Key Behavior**:
- Incomplete JSON at EOF → position unchanged, re-read on next cycle
- Malformed complete JSON → error recorded, position advanced
- Empty lines → skipped silently
- Unknown entry types → `LogEntry::Unknown` via `#[serde(other)]`

### StreamDeserializer Capabilities

**API**: `serde_json::Deserializer::from_str(&content).into_iter::<LogEntry>()`

**Advantages**:
- `byte_offset()` returns exact position after each successful deserialization
- `error.is_eof()` distinguishes incomplete JSON from syntax errors
- `error.classify()` provides `Category::Eof`, `Category::Syntax`, `Category::Data`, `Category::Io`
- Self-delineating JSON values (JSONL compatible)

**Limitations**:
- No file seeking support — must re-parse from start to reach position
- For 100MB file resuming at 50MB: current parser O(1) seek vs StreamDeserializer O(n) re-parse
- Memory usage roughly equivalent for JSONL use case

### POC Implementation Plan

**Location**: Create `src/logs/parser_stream.rs` (separate from main parser)

**Structure**:
```rust
pub struct StreamParseResult {
    pub entries: Vec<DisplayEntry>,
    pub errors: Vec<String>,
    pub bytes_read: u64,
}

pub fn parse_jsonl_stream(path: &Path) -> Result<StreamParseResult>
pub fn parse_jsonl_stream_from_position(path: &Path, position: u64) -> Result<StreamParseResult>
```

**Test Categories** (leverage ct-7hj2 test patterns):

1. **Byte Position Accuracy**
   - Parse file with 5 known entries, verify `byte_offset()` after each
   - Include UTF-8 multi-byte characters (emoji, non-Latin scripts)

2. **Incomplete JSON Detection**
   - File ending with `{"type":"user"` (no closing brace, no newline)
   - Verify `is_eof() == true`, position not advanced

3. **Syntax Error vs EOF Error**
   - Complete malformed line: `{"type":"user" missing close}\n`
   - Verify `is_syntax() == true`, position advanced past line

4. **Error Recovery**
   - File: valid → invalid → valid (all with newlines)
   - Verify 2 entries parsed, 1 error collected, iteration continues

5. **Resumption Simulation**
   - Parse file, record position
   - "Append" new entry (create larger test file)
   - Resume from position, verify only new entry returned

### Benchmark Comparison

Extend `benches/parser.rs` (from ct-ad3b) to include StreamDeserializer variants:

```rust
mod stream_parse {
    #[divan::bench(args = [100, 1000, 10000])]
    fn parse_stream_mixed_entries(bencher: divan::Bencher, count: usize) { ... }
}

mod stream_incremental {
    #[divan::bench(args = [1000, 5000, 10000])]
    fn stream_resume_from_middle(bencher: divan::Bencher, total: usize) { ... }
}
```

**Metrics to capture**:
- Parse time (ns/entry)
- Memory peak (via `#[global_allocator]` tracking if needed)
- Position tracking accuracy (verify against known values)

### Decision Framework

**Proceed with refactoring IF**:
- `byte_offset()` matches expected positions in all test cases
- `is_eof()` correctly identifies incomplete JSON at EOF
- Re-parsing overhead <10% for typical 1-10MB session files
- Code is simpler (fewer manual byte calculations)

**Maintain current approach IF**:
- `byte_offset()` behavior is unreliable or edge-case prone
- Re-parsing penalty >20% for typical file sizes
- Error handling becomes more complex despite `is_eof()` convenience
- Seek-based resumption is critical for larger files

### Deliverables Checklist

- [ ] `src/logs/parser_stream.rs` POC implementation
- [ ] Unit tests covering all 5 test categories
- [ ] Benchmark comparison (extend ct-ad3b)
- [ ] Written analysis document with:
  - Test results table
  - Performance comparison
  - Code complexity comparison
  - Clear recommendation with rationale
- [ ] Decision: PROCEED or MAINTAIN current approach

### Edge Cases to Document

| Edge Case | Expected StreamDeserializer Behavior | Test Method |
|-----------|--------------------------------------|-------------|
| Incomplete JSON at EOF | `is_eof()=true`, position unchanged | Truncated file without newline |
| Malformed complete JSON | `is_syntax()=true`, skip to next | Invalid JSON with trailing newline |
| Empty lines | Skip silently | File with blank lines between entries |
| Very long lines (10KB+) | Parse correctly | Tool call with large input |
| UTF-8 multi-byte | Correct byte counts | Emoji in message content |
| CRLF line endings | Document behavior | Windows-style line endings |
| File truncation | Detect size < position | Rotated log file scenario |

### Notes

- This is research-only; main `parser.rs` remains unchanged
- POC branch recommended for clean separation
- ct-10fv (nextest) can proceed independently; this ticket is not a blocker


## Notes

**2026-02-07T04:40:23Z**

Completed investigation with POC implementation, comprehensive tests, benchmarks, and analysis document. Review agent identified benchmark data errors in initial draft; corrected all performance numbers to use actual mean values from cargo bench. Final recommendation: PROCEED with StreamDeserializer refactoring based on correctness (fixes CRLF bug) and modest performance improvements (5-21% faster).
