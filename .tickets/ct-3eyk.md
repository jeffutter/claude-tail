---
id: ct-3eyk
status: closed
deps: [ct-kkil]
links: []
created: 2026-02-14T23:38:01Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Add comprehensive unit tests for LogEntry types

Add inline test module to src/logs/types.rs covering all LogEntry variants and ContentBlock types.

## Scope (28 tests on ai-slop-refactor)
- All LogEntry variants: User, Assistant, Progress, Unknown/unrecognized
- All ContentBlock types: Text, ToolUse, ToolResult, Thinking, Unknown
- Optional field handling: timestamp, session_id, signature (present and absent)
- Edge cases: empty strings, empty content arrays, mixed block types
- Error cases: missing required fields, invalid JSON, invalid timestamp formats

## Setup
Requires PartialEq derives on ContentValue, ContentBlock, ToolResultContent, ToolResultBlock (included in ct-kkil).

Example test shape:
  #[test]
  fn test_user_message_deserialization() {
      let json = r#'{"type":"user","message":{"role":"user","content":"hello"}}'#;
      let entry: LogEntry = serde_json::from_str(json).unwrap();
      assert!(matches!(entry, LogEntry::User { .. }));
  }

  #[test]
  fn test_unknown_entry_type() {
      let json = r#'{"type":"completely_unknown","foo":"bar"}'#;
      let entry: LogEntry = serde_json::from_str(json).unwrap();
      assert_eq!(entry, LogEntry::Unknown);
  }

## Files
- src/logs/types.rs (inline #[cfg(test)] module)
- Cargo.toml (add tempfile dev-dependency if needed)

## Design

Single-file change: add `#[cfg(test)]` module to `src/logs/types.rs`. No sub-tickets needed — all tests are in one inline module. No new dev-dependencies (serde_json is already available).

### Prerequisite

ct-kkil must be completed first. That ticket:
1. Converts `LogEntry` from a struct to `#[serde(tag = "type")]` enum with `User`, `Assistant`, `Progress`, `Unknown` variants
2. Adds `PartialEq` derives to `ContentValue`, `ContentBlock`, `ToolResultContent`, `ToolResultBlock`

Tests must be written against the **post-refactor** types. The `LogEntry` enum will look like:

```rust
#[serde(tag = "type")]
pub enum LogEntry {
    #[serde(rename = "user")]
    User { message: MessageContent, timestamp: Option<DateTime<Utc>>, session_id: Option<String> },
    #[serde(rename = "assistant")]
    Assistant { message: MessageContent, timestamp: Option<DateTime<Utc>>, session_id: Option<String> },
    #[serde(rename = "progress")]
    Progress { data: serde_json::Value, timestamp: Option<DateTime<Utc>>, session_id: Option<String> },
    #[serde(other)]
    Unknown,
}
```

### Step 1: Add test module scaffold

Add at the bottom of `src/logs/types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    // ... tests
}
```

Use `serde_json::json!` macro with `serde_json::from_value` for constructing test inputs. This is cleaner than raw JSON strings and gives compile-time structure.

### Step 2: Write tests in 5 categories (~28 tests)

**Category 1: LogEntry variant parsing (5 tests)**
- `test_user_variant_full` — User with message, timestamp, session_id
- `test_user_variant_minimal` — User with just `message: {}`
- `test_assistant_variant_minimal` — Assistant with just `message: {}`
- `test_progress_variant` — Progress with data, verify arbitrary JSON preserved
- `test_unknown_variant` — Unrecognized type string deserializes to `Unknown`

Pattern: `serde_json::from_value::<LogEntry>(json)` then `match` on variant, verify fields.

**Category 2: Nested type parsing (8 tests)**
- `test_message_content_text` — ContentValue::Text via User message
- `test_message_content_blocks` — ContentValue::Blocks via Assistant message
- `test_content_block_text` — Direct ContentBlock::Text deserialization
- `test_content_block_tool_use` — ToolUse with id, name, input
- `test_content_block_tool_result_text` — ToolResult with text content
- `test_content_block_tool_result_blocks` — ToolResult with block array content
- `test_content_block_tool_result_error` — ToolResult with `is_error: true`
- `test_content_block_thinking` — Thinking with thinking text and signature
- `test_content_block_unknown` — Unknown block type

Pattern: Deserialize `ContentBlock` directly with `from_value`, match variant, check fields. Use `PartialEq` assertions where types support it.

**Category 3: Optional field handling (5 tests)**
- `test_optional_timestamp_present` — Verify DateTime parsing
- `test_optional_timestamp_missing` — Verify defaults to None
- `test_optional_session_id_present` — Verify string preserved
- `test_optional_session_id_missing` — Verify defaults to None
- `test_optional_signature_in_thinking` — Signature present vs absent

**Category 4: Edge cases (5 tests)**
- `test_empty_message_content` — Empty `{}` message, all fields None
- `test_empty_content_string` — `"content": ""` parses as ContentValue::Text("")
- `test_empty_blocks_array` — `"content": []` parses as ContentValue::Blocks(vec![])
- `test_mixed_known_unknown_blocks` — Array with Text, Unknown, Thinking blocks
- `test_progress_arbitrary_data` — Deeply nested JSON preserved in data field

**Category 5: Error cases (6 tests)**
- `test_missing_type_field` — No "type" key → deserialization error
- `test_invalid_json` — Malformed JSON string → error
- `test_invalid_timestamp_format` — Bad timestamp → error
- `test_tool_use_missing_id` — Required field missing → error
- `test_tool_use_missing_name` — Required field missing → error
- `test_tool_result_missing_tool_use_id` — Required field missing → error

Pattern: `assert!(serde_json::from_value::<T>(json).is_err())`

### Step 3: Verify

- `cargo test --lib` — all new tests pass alongside existing tests
- `cargo clippy` — no warnings

### Scope boundaries

- **Only `src/logs/types.rs` changes** — adding the `#[cfg(test)]` module at the end
- **No Cargo.toml changes** — serde_json is already a dependency, no tempfile needed (tests use in-memory JSON, no file I/O)
- **No behavioral changes** — pure test additions
- **Tests target deserialization only** — they verify serde behavior on the type definitions, not parser logic (that's ct-c0d0)

### Reference

The ai-slop-refactor branch (commit 25c3d8d) has 31 test functions with the same structure. The implementation should follow that pattern but be adapted to whatever the types look like after ct-kkil lands on main. Key difference from reference: the reference has `PartialEq` on `MessageContent` — ct-kkil plan does NOT add `PartialEq` to `MessageContent`, so tests on message fields must use `match` or field-level assertions instead of `assert_eq!` on the whole struct.

## Notes
Source commit: 25c3d8d on ai-slop-refactor. Depends on ct-kkil (tagged enum + PartialEq).

