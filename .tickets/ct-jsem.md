---
id: ct-jsem
status: closed
deps: []
links: []
created: 2026-02-07T01:55:00Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned, testing]
---
# Add unit tests for LogEntry parsing

Add comprehensive unit tests for LogEntry tagged enum deserialization. Should cover all variants (User, Assistant, Progress, Unknown), error handling for malformed JSON, and edge cases like missing optional fields.

## Design

### Location

Add tests inline in `src/logs/types.rs` using a `#[cfg(test)] mod tests { ... }` block at the bottom of the file.

### Test Categories

#### 1. Variant Deserialization (Happy Path)

| Test | JSON Input | Expected |
|------|-----------|----------|
| `test_user_variant_full` | `{"type":"user","message":{...},"timestamp":"...","session_id":"..."}` | User variant with all fields |
| `test_user_variant_minimal` | `{"type":"user","message":{}}` | User variant, optional fields None |
| `test_assistant_variant_full` | `{"type":"assistant","message":{...},"timestamp":"..."}` | Assistant variant |
| `test_assistant_variant_minimal` | `{"type":"assistant","message":{}}` | Assistant variant, optional fields None |
| `test_progress_variant` | `{"type":"progress","data":{...}}` | Progress variant |
| `test_unknown_variant` | `{"type":"future_type",...}` | Unknown variant via `#[serde(other)]` |

#### 2. Nested Type Parsing

| Test | Focus |
|------|-------|
| `test_message_content_text` | ContentValue::Text(String) |
| `test_message_content_blocks` | ContentValue::Blocks(Vec<ContentBlock>) |
| `test_content_block_text` | ContentBlock::Text { text } |
| `test_content_block_tool_use` | ContentBlock::ToolUse { id, name, input } |
| `test_content_block_tool_result_text` | ToolResult with string content |
| `test_content_block_tool_result_blocks` | ToolResult with ToolResultBlock array |
| `test_content_block_tool_result_error` | ToolResult with is_error: true |
| `test_content_block_thinking` | Thinking { thinking, signature } |
| `test_content_block_unknown` | Unknown block type via `#[serde(other)]` |

#### 3. Optional Field Handling

| Test | Scenario |
|------|----------|
| `test_optional_timestamp_present` | Timestamp deserializes correctly |
| `test_optional_timestamp_missing` | Missing timestamp defaults to None |
| `test_optional_session_id_present` | session_id deserializes correctly |
| `test_optional_session_id_missing` | Missing session_id defaults to None |
| `test_optional_signature_in_thinking` | Thinking block with/without signature |

#### 4. Edge Cases

| Test | Scenario |
|------|----------|
| `test_empty_message_content` | `{"type":"user","message":{}}` - all inner fields None |
| `test_empty_content_string` | `"content": ""` - empty text |
| `test_empty_blocks_array` | `"content": []` - empty array |
| `test_mixed_known_unknown_blocks` | Array with Text and unknown block types |
| `test_progress_arbitrary_data` | Progress.data handles any JSON structure |

#### 5. Error Cases

| Test | Scenario | Expected |
|------|----------|----------|
| `test_missing_type_field` | `{"message":{}}` | Deserialization error |
| `test_invalid_json` | `{not valid json}` | Parse error |
| `test_invalid_timestamp_format` | `"timestamp": "not-a-date"` | Deserialization error |
| `test_tool_use_missing_id` | ToolUse without required `id` | Deserialization error |
| `test_tool_use_missing_name` | ToolUse without required `name` | Deserialization error |
| `test_tool_result_missing_tool_use_id` | ToolResult without required field | Deserialization error |

### Implementation Pattern

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_user_variant_full() {
        let json = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": "Hello"
            },
            "timestamp": "2026-02-06T19:30:00Z",
            "session_id": "session-123"
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::User { message, timestamp, session_id } => {
                assert_eq!(message.role, Some("user".to_string()));
                assert!(timestamp.is_some());
                assert_eq!(session_id, Some("session-123".to_string()));
            }
            _ => panic!("Expected User variant"),
        }
    }

    // ... additional tests follow same pattern
}
```

### Acceptance Criteria

- [ ] All 4 LogEntry variants have basic deserialization tests
- [ ] All ContentBlock variants tested (Text, ToolUse, ToolResult, Thinking, Unknown)
- [ ] Optional fields tested for both presence and absence
- [ ] Edge cases covered (empty strings, empty arrays, mixed blocks)
- [ ] Error cases verify deserialization fails appropriately
- [ ] Tests run with `cargo test` (standard test harness)

### Notes

- No dev-dependencies needed; `serde_json::json!` macro already available
- CI already configured to run `cargo test --all-features --workspace`
- This is the first test module in the project; establishes patterns for ct-7hj2

