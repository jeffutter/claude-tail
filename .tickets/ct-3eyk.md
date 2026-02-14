---
id: ct-3eyk
status: open
deps: [ct-kkil]
links: []
created: 2026-02-14T23:38:01Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [needs-plan]
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

## Notes
Source commit: 25c3d8d on ai-slop-refactor. Depends on ct-kkil (tagged enum + PartialEq). Re-planning required since types may differ.

