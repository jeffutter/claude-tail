---
id: ct-5vmd
status: open
deps: [ct-kkil]
links: []
created: 2026-02-14T23:37:52Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Refactor JSONL parser to use StreamDeserializer

Replace the current line-based JSONL parsing approach with serde_json::StreamDeserializer. Primary motivation is a CRLF bug fix; secondary benefit is cleaner code and modest performance improvement.

## Problem
Current line-based parser in src/logs/parser.rs iterates lines via content.lines(), manually tracking byte offsets. This has a bug: CRLF line endings (\r\n) cause silent position undercounting because .lines() strips \r but the manual offset math doesn't account for it.

## Target approach
Use serde_json::Deserializer::from_str() with into_iter::<LogEntry>() (StreamDeserializer). Key improvements:
  - Automatic byte position tracking via stream.byte_offset()
  - Cleaner EOF detection via error.classify() returning Category::Eof vs Category::Syntax
  - Eliminates manual newline/CRLF accounting
  - ~3-5% performance improvement

Core parsing loop changes from line iteration to:
  while current_pos < content.len() {
      let slice = &content[current_pos..];
      let deserializer = serde_json::Deserializer::from_str(slice);
      let mut stream = deserializer.into_iter::<LogEntry>();
      match stream.next() {
          Some(Ok(entry)) => {
              entries.extend(convert_log_entry(&entry));
              current_pos += stream.byte_offset();
              // skip trailing whitespace
          }
          Some(Err(e)) if e.classify() == Category::Eof => break,
          Some(Err(e)) => { errors.push(...); advance past error... }
          None => break,
      }
  }

Error messages change from 'Line N: ...' to 'Parse error at byte N: ...'.

## Files
- src/logs/parser.rs

## Design

Pure refactor of parsing logic — no public API changes, no new dependencies. Single file change: `src/logs/parser.rs`.

### Prerequisite

ct-kkil must be completed first. That ticket changes `LogEntry` from a struct to a `#[serde(tag = "type")]` enum and updates `convert_log_entry` to use exhaustive pattern matching. This plan assumes the tagged enum is in place.

### Step 1: Add `use serde_json::error::Category` import

Add to the imports at the top of parser.rs:
```rust
use serde_json::error::Category;
```

No new crate dependencies needed — `Category` is already in `serde_json = "1.0"`.

### Step 2: Extract `parse_stream_content` private function

Add a new private function that consolidates the duplicated parsing logic from `parse_jsonl_file` and `parse_jsonl_from_position` into a single function:

```rust
fn parse_stream_content(content: &str, base_position: u64) -> Result<ParseResult> {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    let mut last_valid_position = 0;
    let mut current_pos = 0;

    while current_pos < content.len() {
        let slice = &content[current_pos..];
        let deserializer = serde_json::Deserializer::from_str(slice);
        let mut stream = deserializer.into_iter::<LogEntry>();

        match stream.next() {
            Some(Ok(entry)) => {
                entries.extend(convert_log_entry(&entry));
                let offset = stream.byte_offset();
                current_pos += offset;

                // Skip trailing whitespace (including newlines)
                let remaining = &content[current_pos..];
                let whitespace_len = remaining
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .map(|c| c.len_utf8())
                    .sum::<usize>();
                current_pos += whitespace_len;
                last_valid_position = current_pos;
            }
            Some(Err(e)) => {
                let error_offset = current_pos + stream.byte_offset();
                match e.classify() {
                    Category::Eof => break,
                    Category::Syntax | Category::Data => {
                        errors.push(format!(
                            "Parse error at byte {}: {}",
                            base_position + error_offset as u64, e
                        ));
                        // Recover: skip to next newline
                        if let Some(remaining) = slice.get(stream.byte_offset()..) {
                            if let Some(newline_pos) = remaining.find('\n') {
                                current_pos = current_pos + stream.byte_offset() + newline_pos + 1;
                                last_valid_position = current_pos;
                            } else {
                                last_valid_position = content.len();
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    Category::Io => {
                        return Err(anyhow::anyhow!("I/O error during deserialization: {}", e));
                    }
                }
            }
            None => break,
        }
    }

    Ok(ParseResult {
        entries,
        errors,
        bytes_read: base_position + last_valid_position as u64,
    })
}
```

Key design decisions:
- **`last_valid_position`** tracks the furthest position we've fully consumed (vs `current_pos` which may be mid-parse). This is what becomes `bytes_read`.
- **`base_position`** parameter lets error messages report absolute byte offsets in the file.
- **Whitespace skipping** uses `.chars().take_while(|c| c.is_whitespace()).map(|c| c.len_utf8())` for UTF-8-safe handling of newlines and CRLF.
- **EOF errors** (`Category::Eof`) break without recording an error — this preserves the existing behavior where incomplete JSON at EOF is silently ignored for re-reading on next incremental parse.
- **Syntax/Data errors** skip to the next newline and continue — preserves the existing error recovery behavior.
- **No newline at EOF with malformed data**: sets `last_valid_position = content.len()` so the position advances past the bad data (matches current behavior where a complete line with a parse error advances `bytes_consumed`).

### Step 3: Simplify `parse_jsonl_file` and `parse_jsonl_from_position`

Replace the bodies of both functions to delegate to `parse_stream_content`:

```rust
pub fn parse_jsonl_file(path: &Path) -> Result<ParseResult> {
    let mut file = std::fs::File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    parse_stream_content(&content, 0)
}

pub fn parse_jsonl_from_position(path: &Path, position: u64) -> Result<ParseResult> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(position))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    parse_stream_content(&content, position)
}
```

This eliminates ~50 lines of duplicated line-iteration logic.

### Step 4: Remove unused import

The `std::io::Read` import is still needed (for `read_to_string`). No import changes beyond the `Category` addition.

### Step 5: Verify

- `cargo check` — confirms type safety
- `cargo clippy` — no new warnings
- `cargo test --lib` — all existing unit tests pass
- `cargo test --test scrolling_tests` — integration tests pass
- Manual smoke test: run against real JSONL logs, verify conversation renders identically

### Scope boundaries

- **Only `src/logs/parser.rs` changes.** No other files.
- **Public API unchanged**: `parse_jsonl_file`, `parse_jsonl_from_position`, async variants, `merge_tool_results`, `ParseResult` — all identical signatures and behavior.
- **No `parse_jsonl_range`**: The reference branch adds range parsing for windowed buffer support, but that's not in scope for this ticket. It can be added later if needed.
- **Error message format changes**: From `"Line N: {error}"` / `"Incremental line N: {error}"` to `"Parse error at byte N: {error}"`. These error strings are stored in `ParseResult.errors` and displayed in `app.rs` via `parse_errors`. This is a minor visible change but an improvement (byte offsets are more useful for debugging than line numbers in JSONL).

### Behavioral equivalence

The StreamDeserializer approach preserves all existing behaviors:
1. Incomplete JSON at EOF → not consumed, no error recorded → re-read on next incremental parse
2. Malformed JSON on complete line → error recorded, position advances past it
3. Empty/whitespace lines → skipped (handled by whitespace-skipping loop)
4. `bytes_read` → absolute file position for both initial and incremental parses
5. Multiple entries → all parsed and converted via `convert_log_entry`

The one bug fix this introduces: CRLF line endings are now handled correctly because `byte_offset()` and the whitespace-skipping loop account for `\r\n` automatically, whereas the current manual `content.as_bytes().get(line_end) == Some(&b'\n')` check only looks for `\n`.

## Notes
Source commit: 7842d26 on ai-slop-refactor. Depends on LogEntry tagged enum (ct-kkil) being implemented first as the pattern matching in convert_log_entry will differ.

