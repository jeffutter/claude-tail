---
id: ct-x4mf
status: closed
deps: []
links: []
created: 2026-02-07T05:04:19Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned, jsonl, parser, refactoring]
---
# Refactor JSONL parser to use StreamDeserializer

Replace the current line-based JSONL parser (src/logs/parser.rs) with StreamDeserializer approach.

Primary justification: Fixes CRLF bug where Windows line endings cause silent position tracking errors (undercounts by number of \r characters).

Additional benefits:
- Clearer EOF detection via error.classify() instead of newline boundary heuristics  
- Automatic byte position tracking via byte_offset() (eliminates manual byte math)
- Minor performance improvements (3-5% typical case)

Implementation:
- Replace parser.rs implementation with parser_stream.rs approach (POC exists)
- Keep existing async wrappers unchanged
- Add CRLF regression test
- All 28 existing tests should continue passing

Risk: LOW - POC implementation passes all tests, produces identical ParseResult structures, no API changes required.

## Design

### Approach: In-place replacement of parser.rs internals

The POC in `parser_stream.rs` validates the StreamDeserializer approach. This task replaces the parsing logic inside `parser.rs` while preserving all public API signatures. No sub-tickets needed — this is a focused, single-session refactoring.

### Preconditions

- All prerequisites satisfied (ct-jsem, ct-7hj2, ct-ad3b, ct-6qk4 all closed)
- POC in `parser_stream.rs` passes 16 tests and produces identical `ParseResult` structures
- Existing test suite (28 tests in `parser.rs`) provides regression safety net
- Benchmarks in `benches/parser.rs` provide performance baseline

### Steps

**Step 1: Replace parsing core in parser.rs**

Replace the line-based parsing logic in `parse_jsonl_file()` and `parse_jsonl_from_position()` with the StreamDeserializer approach from `parser_stream.rs`:

- Add `use serde_json::error::Category;` import
- Extract `parse_stream_content()` as a private helper (from POC lines 41-110)
- Rewrite `parse_jsonl_file()` to call `parse_stream_content(&content, 0)` instead of the `content.lines()` loop
- Rewrite `parse_jsonl_from_position()` to call `parse_stream_content(&content, position)` after seeking
- Keep `ParseResult` struct unchanged (no rename needed — POC's `StreamParseResult` is structurally identical)
- Keep `convert_log_entry()`, `merge_tool_results()`, and both `_async` wrappers unchanged

Key logic to port (from `parser_stream.rs:47-103`):
```
while current_pos < content.len() {
    create Deserializer from &content[current_pos..]
    match stream.next():
        Ok(entry)  → extend entries, advance via byte_offset()
        Err(Eof)   → break (incomplete JSON, hold position)
        Err(Syntax/Data) → record error, skip to next newline
        Err(Io)    → return error
        None       → break
}
```

**Step 2: Fix POC issue — malformed JSON at EOF without newline**

The POC (lines 83-86) sets `last_valid_position = content.len()` when malformed JSON has no trailing newline. This advances position past the bad data, which is correct for malformed data (unlike incomplete JSON at EOF, which should hold position). Verify this matches the existing parser's behavior and adjust if needed.

**Step 3: Update or remove whitespace/empty line handling**

Current parser explicitly skips empty/whitespace lines (`if line.trim().is_empty() { continue }`). StreamDeserializer handles this implicitly — JSON whitespace between values is consumed by the deserializer. Verify no position tracking differences arise from this change.

**Step 4: Update existing tests**

- Fix `test_crlf_line_endings` — change the expected `bytes_read` from 58 to the correct value (59) since StreamDeserializer correctly counts `\r` bytes
- Verify all other 27 tests pass without modification
- If any tests encode line-based assumptions (e.g., error messages referencing "line N"), update error message format to use byte offsets instead

**Step 5: Add CRLF regression test**

Add a dedicated test that verifies CRLF position tracking is correct:
- Write multiple JSONL entries with `\r\n` line endings
- Assert `bytes_read` accounts for all `\r` characters
- Test incremental parsing (resume from position) with CRLF content

**Step 6: Remove parser_stream.rs**

After all tests pass with the new implementation:
- Delete `src/logs/parser_stream.rs`
- Remove `pub mod parser_stream;` from `src/logs/mod.rs`
- The POC's 16 tests can be dropped — the existing 28+ tests in `parser.rs` cover the same ground, and the CRLF test is now correct

**Step 7: Run benchmarks and validate**

- Run `cargo bench` to compare against baseline
- Expect 3-8% improvement in typical cases (do not expect the inflated numbers from the original analysis)
- Verify no regression at any error rate
- Run `cargo clippy` and `cargo fmt` for cleanliness

### What stays unchanged

- `ParseResult` struct (same fields: entries, errors, bytes_read)
- `parse_jsonl_file_async()` and `parse_jsonl_from_position_async()` signatures and behavior
- `merge_tool_results()` — post-parse transformation, unrelated to parsing approach
- `convert_log_entry()` — LogEntry → DisplayEntry conversion, unrelated
- All callers in `app.rs` — no API changes visible to consumers
- File watcher in `watcher.rs` — no changes needed
- Module exports in `mod.rs` — same public API (minus `parser_stream` removal)

### Acceptance criteria

- [ ] All existing parser tests pass (28 tests)
- [ ] CRLF test asserts correct byte position (not the buggy value)
- [ ] New CRLF regression test with multi-line incremental parsing
- [ ] `parser_stream.rs` removed (POC code absorbed into parser.rs)
- [ ] `cargo clippy` clean
- [ ] `cargo bench` shows no regression
- [ ] Manual smoke test: launch app, load a session, verify conversation renders correctly

## Notes

**2026-02-07T05:04:39Z**

Ticket created from ct-6qk4 research. Marked for detailed planning in separate pass.
