---
id: ct-kkil
status: open
deps: []
links: []
created: 2026-02-14T23:37:26Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [needs-plan]
---
# Refactor LogEntry to tagged enum with PartialEq derives

Replace the current LogEntry struct (with optional fields and string-based type checking) with a proper Serde tagged enum. Also add PartialEq derives to content types to enable test assertions.

## Background
Currently src/logs/types.rs has a struct with an entry_type String field and all fields optional:
  pub struct LogEntry {
      pub entry_type: String,
      pub message: Option<MessageContent>,
      pub data: Option<serde_json::Value>,
      ...
  }

And parser.rs matches on the string:
  match entry.entry_type.as_str() {
      "user" => { if let Some(ref message) = entry.message { ... } }
      "progress" => { if let Some(ref data) = entry.data { ... } }
      _ => {}
  }

## Target
Replace with a proper tagged enum so each variant carries exactly the fields it needs:
  #[serde(tag = "type")]
  pub enum LogEntry {
      #[serde(rename = "user")]
      User { message: MessageContent, timestamp: Option<DateTime<Utc>>, ... },
      #[serde(rename = "assistant")]
      Assistant { message: MessageContent, ... },
      #[serde(rename = "progress")]
      Progress { data: serde_json::Value, ... },
      #[serde(other)]
      Unknown,
  }

Parser becomes a clean exhaustive match with no Option unwrapping.

Also add PartialEq to ContentValue, ContentBlock, ToolResultContent, and ToolResultBlock.

## Files
- src/logs/types.rs
- src/logs/parser.rs

## Notes
Pure refactor, no functional change. Source commit: 306e27c on ai-slop-refactor. Codebase has evolved so re-planning is required.

