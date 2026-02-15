---
id: ct-c0d0
status: closed
deps: [ct-kkil, ct-5vmd]
links: []
created: 2026-02-14T23:38:12Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Add comprehensive unit tests for JSONL parser

Add inline test module to src/logs/parser.rs covering byte position tracking, incremental parsing, error recovery, and edge cases.

## Scope (29 tests on ai-slop-refactor, across 5 categories)
1. Byte position tracking (6 tests): single entry, multiple entries, empty lines, trailing newline, no trailing newline
2. Resume from position (5 tests): from start, from middle, from end, incremental append simulation
3. Partial/incomplete lines at EOF (5 tests): truncated JSON, partial line with no newline, mid-stream append
4. Error recovery (7 tests): malformed JSON on one line, multiple bad lines, bad line followed by good, mixed
5. Edge cases (5 tests): UTF-8 multi-byte characters, CRLF line endings, very long lines, empty file, whitespace-only

## Key behavior to verify
- parse_jsonl_from_position() resumes correctly without re-reading already-consumed bytes
- Incomplete JSON at EOF is NOT treated as an error (just not consumed)
- CRLF (\r\n) endings are handled correctly in byte position math (was buggy before StreamDeserializer)
- Error lines are skipped and reported but don't abort parsing

Example:
  #[test]
  fn test_resume_from_position() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("test.jsonl");
      // write first entry, parse it, get bytes_read
      // append second entry, parse from bytes_read
      // verify only second entry returned
  }

## Files
- src/logs/parser.rs (inline #[cfg(test)] module)
- Cargo.toml (add tempfile dev-dependency)

## Design

No sub-tickets needed — all tests go in a single `#[cfg(test)]` module in `src/logs/parser.rs`. Two files change: `parser.rs` and `Cargo.toml`.

### Prerequisites

Both ct-kkil and ct-5vmd must be completed first:
- **ct-kkil** converts `LogEntry` from a struct with `entry_type: String` to a `#[serde(tag = "type")]` enum with `User`, `Assistant`, `Progress`, `Unknown` variants. Tests construct JSONL strings matching this format.
- **ct-5vmd** replaces line-based parsing with `StreamDeserializer`. This changes:
  - Byte position tracking uses `stream.byte_offset()` instead of manual newline math
  - CRLF line endings are handled correctly (the current line-based parser has a bug where `\r` is stripped by `.lines()` but not accounted for in byte math)
  - Error messages change from `"Line N: {error}"` to `"Parse error at byte N: {error}"`
  - Incomplete JSON at EOF triggers `Category::Eof` and breaks cleanly (not consumed, no error)
  - A private `parse_stream_content(content: &str, base_position: u64) -> Result<ParseResult>` function consolidates parsing logic

Tests must be written against the **post-refactor** API. The public functions remain the same: `parse_jsonl_file(&Path)`, `parse_jsonl_from_position(&Path, u64)`, and `merge_tool_results(Vec<DisplayEntry>)`.

### Step 1: Add tempfile dev-dependency to Cargo.toml

```toml
[dev-dependencies]
tempfile = "3"
```

If ct-jfxa (divan benchmarks) has already executed, `tempfile` may already be present — just verify.

### Step 2: Add test module scaffold to parser.rs

At the bottom of `src/logs/parser.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Helper: generate a user entry JSONL line
    fn user_entry(text: &str) -> String {
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": text}
        }).to_string()
    }

    // Helper: generate an assistant entry JSONL line
    fn assistant_entry(text: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": text}
        }).to_string()
    }

    // Helper: generate a progress entry JSONL line
    fn progress_entry() -> String {
        serde_json::json!({
            "type": "progress",
            "data": {"message": {"role": "assistant", "content": [{"type": "text", "text": "thinking..."}]}}
        }).to_string()
    }

    // ... tests below
}
```

All tests write to `NamedTempFile` and call the public parse functions. This exercises the real file I/O path. `DisplayEntry` does NOT have `PartialEq`, so assertions use `matches!()` macro or field-level destructuring.

### Step 3: Write tests in 5 categories (~29 tests)

**Category 1: Byte position tracking (6 tests)**

These verify that `bytes_read` in `ParseResult` accurately reflects the number of bytes consumed.

- `test_single_entry_position` — Write one entry + newline, verify `bytes_read == line.len() + 1`
- `test_multiple_entries_position` — Write 3 entries, verify cumulative position
- `test_empty_lines_position` — Entries separated by empty lines, verify empty lines are counted in position
- `test_trailing_newline_position` — File ends with `\n`, position includes it
- `test_no_trailing_newline_position` — File ends without `\n`, verify position still correct (after ct-5vmd, whitespace skipping handles this)
- `test_whitespace_only_lines_position` — Lines with only spaces/tabs between entries

**Category 2: Resume from position (5 tests)**

These verify `parse_jsonl_from_position` correctly resumes mid-file.

- `test_resume_from_start` — Position 0, should parse entire file (same as `parse_jsonl_file`)
- `test_resume_from_middle` — Parse first half, get `bytes_read`, resume from there, verify only second half returned
- `test_resume_from_end` — Position at file end, returns empty entries with position unchanged
- `test_incremental_append` — Parse file, append new entry, resume from `bytes_read`, verify only new entry returned
- `test_resume_multiple_increments` — Three rounds of append+resume, verify each round returns only new entries

Pattern for resume tests:
```rust
#[test]
fn test_resume_from_middle() {
    let mut file = NamedTempFile::new().unwrap();
    let line1 = user_entry("first");
    let line2 = user_entry("second");
    writeln!(file, "{}", line1).unwrap();
    writeln!(file, "{}", line2).unwrap();
    file.flush().unwrap();

    // Parse just the first entry by parsing full file and checking position
    let result1 = parse_jsonl_file(file.path()).unwrap();
    assert_eq!(result1.entries.len(), 2);

    // Resume from after first entry
    let first_entry_end = (line1.len() + 1) as u64;
    let result2 = parse_jsonl_from_position(file.path(), first_entry_end).unwrap();
    assert_eq!(result2.entries.len(), 1);
    assert!(matches!(&result2.entries[0], DisplayEntry::UserMessage { text, .. } if text == "second"));
}
```

**Category 3: Partial/incomplete lines at EOF (5 tests)**

After ct-5vmd, incomplete JSON at EOF triggers `Category::Eof` and is NOT consumed (no error recorded, position stays before the incomplete data).

- `test_truncated_json_at_eof` — Write partial JSON without newline (e.g., `{"type":"us`), verify 0 entries, 0 errors, position stays at 0
- `test_complete_then_truncated` — Valid entry + truncated JSON at EOF, verify 1 entry, 0 errors, position after the valid entry only
- `test_incomplete_append_then_complete` — Write truncated entry, parse (0 entries). Append rest of entry + newline, resume from position 0, verify 1 entry.
- `test_partial_line_no_newline` — Complete JSON without trailing newline, verify it IS consumed (StreamDeserializer can parse complete JSON without newline)
- `test_mid_stream_growth` — Simulate streaming: parse, append, resume, parse, append, resume — verify cumulative entries match

**Category 4: Error recovery (7 tests)**

After ct-5vmd, errors report byte offsets instead of line numbers. Malformed JSON on a complete line is recorded as an error and skipped.

- `test_single_malformed_line` — One bad line, verify 0 entries, 1 error
- `test_malformed_between_valid` — Valid + malformed + valid, verify 2 entries, 1 error
- `test_multiple_malformed_lines` — 3 bad lines interleaved with 2 valid, verify 2 entries, 3 errors
- `test_errors_contain_byte_offsets` — Verify error strings contain "Parse error at byte" with approximate offset
- `test_errors_dont_stop_parsing` — Bad entry early in file, many valid after, verify all valid entries parsed
- `test_empty_lines_not_errors` — Empty/whitespace lines are NOT counted as errors
- `test_unknown_type_not_error` — `{"type":"future_type","data":{}}` deserializes as `Unknown` variant, produces 0 entries but 0 errors

**Category 5: Edge cases (5 tests)**

- `test_utf8_multibyte_characters` — Entries with Japanese, emoji, accented chars. Verify correct entry count and byte position (JSON-escaped multi-byte chars have different byte lengths)
- `test_crlf_line_endings` — Write entries with `\r\n` instead of `\n`. After ct-5vmd, position should account for both bytes correctly. Verify `bytes_read` includes `\r` bytes.
- `test_very_long_line` — Entry with 15KB text content, verify it parses without error
- `test_empty_file` — Empty file, verify 0 entries, 0 errors, `bytes_read == 0`
- `test_mixed_valid_unknown_types` — User + unknown_type + assistant entries, verify 2 display entries (unknown produces none), 0 errors

### Step 4: Verify

1. `cargo check` — type safety
2. `cargo clippy` — no warnings
3. `cargo test --lib` — all new + existing tests pass
4. `cargo test --test scrolling_tests` — integration tests unaffected

### Scope boundaries

- **Two files change**: `src/logs/parser.rs` (add `#[cfg(test)]` module), `Cargo.toml` (add `tempfile` dev-dependency)
- **No behavioral changes** — pure test additions
- **Tests exercise public API only**: `parse_jsonl_file`, `parse_jsonl_from_position`, `merge_tool_results`
- **No PartialEq needed on DisplayEntry**: All assertions use `matches!()` macro with guard patterns or destructure and check fields individually
- **Tests must be adapted to post-refactor state**: Written against tagged enum LogEntry (ct-kkil) and StreamDeserializer (ct-5vmd). If either dependency changes during implementation, the test data format or error message assertions may need adjustment.

### Adaptation notes for CRLF test

The current line-based parser has a known CRLF bug (position undercounts by the number of `\r` characters). After ct-5vmd, this is fixed. The CRLF test should verify **correct** behavior:

```rust
#[test]
fn test_crlf_line_endings() {
    let mut file = NamedTempFile::new().unwrap();
    let line = user_entry("test");
    write!(file, "{}\r\n", line).unwrap();
    file.flush().unwrap();

    let result = parse_jsonl_file(file.path()).unwrap();
    assert_eq!(result.entries.len(), 1);
    // After StreamDeserializer: position includes both \r and \n
    assert_eq!(result.bytes_read, (line.len() + 2) as u64);
}
```

## Notes
Source commits: d510a39 (initial tests) + fixes in 7842d26 (CRLF test correction) on ai-slop-refactor. Depends on ct-kkil (tagged enum) and ct-5vmd (StreamDeserializer, for correct CRLF behavior).


**2026-02-15T07:20:25Z**

Implemented 33 tests (plan called for 29): 6 byte-position, 5 resume, 5 partial-EOF, 7 error-recovery, 5 edge-cases, 3 bonus merge_tool_results tests. Renamed misplaced progress-entry test, added intervening-entry merge test, and strengthened byte-offset assertion per review feedback. All 65 unit tests pass, clippy clean.
