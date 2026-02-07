---
id: ct-7hj2
status: open
deps: []
links: []
created: 2026-02-07T01:55:03Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned, testing]
---
# Add tests for incremental JSONL parsing

Test the incremental parsing mechanism that tracks byte positions and handles incomplete lines at EOF. Must verify: byte position tracking across multiple reads, resuming from position, handling of partial lines when file is being actively written to, and error recovery.

## Design

### Location

Add tests inline in `src/logs/parser.rs` using a `#[cfg(test)] mod tests { ... }` block at the bottom of the file. Follow the same pattern established in ct-jsem for `src/logs/types.rs`.

### Test Infrastructure

Create a helper module for generating test JSONL content:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Helper to create valid JSONL entry
    fn user_entry(text: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#, text)
    }

    fn assistant_entry(text: &str) -> String {
        format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":"{}"}}}}"#, text)
    }
}
```

**Dependency needed**: Add `tempfile = "3"` to `[dev-dependencies]` in Cargo.toml for creating temporary test files.

### Test Categories

#### 1. Byte Position Tracking (Core Correctness)

| Test | Scenario | Verification |
|------|----------|--------------|
| `test_position_empty_file` | Parse empty file | `bytes_read == 0`, no entries |
| `test_position_single_line` | File: `{json}\n` | `bytes_read == line.len() + 1` (includes newline) |
| `test_position_multiple_lines` | 3 lines with newlines | `bytes_read == sum(line.len() + 1)` for each line |
| `test_position_no_trailing_newline` | File: `{json}` (no `\n`) | `bytes_read == 0` (incomplete line not consumed) |
| `test_position_with_utf8` | JSON with emoji: `"text":"hello 😀"` | Position accounts for 4-byte UTF-8 correctly |
| `test_position_accumulates_correctly` | Parse file, get pos P1. Parse from P1, get P2. | `P2 == P1 + delta` (absolute positioning) |

#### 2. Resuming from Position

| Test | Scenario | Verification |
|------|----------|--------------|
| `test_resume_from_zero` | Parse from position 0 | Same result as `parse_jsonl_file` |
| `test_resume_from_middle` | Parse full → get pos → append line → parse from pos | Only new line returned |
| `test_resume_position_beyond_eof` | Seek to position > file length | Returns empty entries, no error |
| `test_resume_incremental_accumulation` | 3 sequential appends with parse after each | Each parse returns only new entries |
| `test_resume_preserves_position_on_empty_read` | Parse from pos at exact EOF | `bytes_read == original_pos`, no entries |

#### 3. Partial/Incomplete Lines at EOF

| Test | Scenario | Verification |
|------|----------|--------------|
| `test_incomplete_json_not_consumed` | File: `{"type":"user"` (no closing brace, no newline) | `bytes_read == 0`, no entries, no errors |
| `test_incomplete_then_complete` | Write partial → parse → append rest + newline → parse | Second parse gets complete entry |
| `test_complete_json_no_newline` | File: `{valid_json}` (complete JSON, no trailing newline) | Should NOT consume (waits for newline) |
| `test_multiple_lines_last_incomplete` | 2 complete lines + 1 incomplete | First 2 parsed, position after line 2 |
| `test_complete_line_followed_by_incomplete` | `{line1}\n{partial` | `bytes_read` points after line 1's newline |

#### 4. Error Recovery

| Test | Scenario | Verification |
|------|----------|--------------|
| `test_malformed_json_complete_line` | `{invalid json}\n` | Error recorded, position advances past line |
| `test_malformed_json_incomplete_line` | `{invalid json` (no newline) | No error yet, position stays at 0 |
| `test_errors_dont_stop_parsing` | Valid → Invalid → Valid (all with newlines) | 2 entries parsed, 1 error recorded |
| `test_empty_lines_skipped` | `{json1}\n\n\n{json2}\n` | 2 entries, position includes empty line bytes |
| `test_whitespace_only_lines` | `{json}\n   \t  \n{json2}\n` | Whitespace lines skipped, 2 entries parsed |
| `test_errors_contain_line_numbers` | Invalid JSON at line 3 | Error message contains "Line 3" |

#### 5. Edge Cases

| Test | Scenario | Verification |
|------|----------|--------------|
| `test_very_long_line` | Single JSON line > 10KB | Parses correctly, position accurate |
| `test_special_characters_in_json` | JSON with `\n`, `\t`, `\"` in string values | Not confused by escaped newlines |
| `test_unicode_boundary_safe` | Multi-byte UTF-8 at various positions | Position calculation handles byte lengths |
| `test_crlf_line_endings` | File with `\r\n` instead of `\n` | **Document behavior** (may expose bug) |
| `test_mixed_valid_unknown_types` | Mix of User, Assistant, Unknown types | All parsed, Unknown handled gracefully |

### Implementation Pattern

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn user_entry(text: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#, text)
    }

    #[test]
    fn test_position_single_line() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("hello");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        // line + newline = bytes consumed
        assert_eq!(result.bytes_read, (line.len() + 1) as u64);
    }

    #[test]
    fn test_resume_from_middle() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");

        // Write first line, parse, get position
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();
        let result1 = parse_jsonl_file(file.path()).unwrap();
        let pos = result1.bytes_read;

        // Append second line
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        // Resume from saved position
        let result2 = parse_jsonl_from_position(file.path(), pos).unwrap();

        assert_eq!(result2.entries.len(), 1); // Only the new line
        assert_eq!(result2.bytes_read, pos + (line2.len() + 1) as u64);
    }

    #[test]
    fn test_incomplete_json_not_consumed() {
        let mut file = NamedTempFile::new().unwrap();
        // Write incomplete JSON (no closing brace, no newline)
        write!(file, r#"{{"type":"user","message":{{"#).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert!(result.entries.is_empty());
        assert!(result.errors.is_empty()); // No error for incomplete line
        assert_eq!(result.bytes_read, 0);  // Position not advanced
    }
}
```

### Acceptance Criteria

- [ ] All 5 test categories implemented (25+ tests total)
- [ ] Tests cover both `parse_jsonl_file` and `parse_jsonl_from_position`
- [ ] Position tracking verified for single-line, multi-line, and incremental scenarios
- [ ] Incomplete line handling verified (parser waits for newline)
- [ ] Error recovery tested (malformed JSON doesn't crash, position advances correctly)
- [ ] Edge cases covered (UTF-8, long lines, empty lines)
- [ ] `tempfile` added to dev-dependencies
- [ ] All tests pass with `cargo test`

### Notes

- CR/LF test (`test_crlf_line_endings`) may expose a bug—parser only checks for `\n`, not `\r\n`. Document current behavior; fixing is out of scope for this ticket.
- The async wrappers (`parse_jsonl_file_async`, `parse_jsonl_from_position_async`) are thin wrappers and don't need separate tests.
- Integration with `SessionWatcher` is not tested here—that's higher-level integration testing.
- Follow ct-jsem's pattern: inline tests in the module file, use `serde_json::json!` macro where helpful.

