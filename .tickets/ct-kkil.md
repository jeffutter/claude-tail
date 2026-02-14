---
id: ct-kkil
status: open
deps: []
links: []
created: 2026-02-14T23:37:26Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned]
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

## Design

Pure refactor — no functional change, no new dependencies.

### Step 1: Add PartialEq derives to content types (types.rs)

Add `PartialEq` to the derive macros on these four types:
- `ContentValue` (line 28) — untagged enum, both variants (String, Vec) support PartialEq
- `ContentBlock` (line 35) — tagged enum; note `ToolUse.input` is `serde_json::Value` which implements PartialEq
- `ToolResultContent` (line 64) — untagged enum, same as ContentValue
- `ToolResultBlock` (line 71) — struct with String and Option<String>

Do NOT add PartialEq to `LogEntry` (Progress variant holds `serde_json::Value` — while Value does implement PartialEq, keeping it off LogEntry is fine since downstream test tickets don't need it on the enum itself), `MessageContent`, `ToolCallResult`, or `DisplayEntry`.

### Step 2: Replace LogEntry struct with tagged enum (types.rs)

Replace the struct at lines 4-16:

```rust
// Before
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(default)]
    pub message: Option<MessageContent>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub session_id: Option<String>,
}

// After
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LogEntry {
    #[serde(rename = "user")]
    User {
        message: MessageContent,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
        #[serde(default)]
        session_id: Option<String>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        message: MessageContent,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
        #[serde(default)]
        session_id: Option<String>,
    },
    #[serde(rename = "progress")]
    Progress {
        data: serde_json::Value,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
        #[serde(default)]
        session_id: Option<String>,
    },
    #[serde(other)]
    Unknown,
}
```

Key decisions:
- `message` is non-optional in User/Assistant (the JSON always has it when type is user/assistant)
- `data` is non-optional in Progress (same reasoning)
- `timestamp` and `session_id` remain optional with `#[serde(default)]` on all variants
- `#[serde(other)]` catches unknown type values — matches existing `ContentBlock::Unknown` pattern
- `Unknown` variant has no fields (serde limitation with `#[serde(other)]`)

### Step 3: Update convert_log_entry (parser.rs)

Replace the function at lines 132-156:

```rust
// Before: string matching + Option unwrapping
fn convert_log_entry(entry: &LogEntry) -> Vec<DisplayEntry> {
    let mut display_entries = Vec::new();
    let timestamp = entry.timestamp;
    match entry.entry_type.as_str() {
        "user" => {
            if let Some(ref message) = entry.message {
                display_entries.extend(parse_user_message(message, timestamp));
            }
        }
        ...
    }
    display_entries
}

// After: exhaustive pattern match, no Option unwrapping
fn convert_log_entry(entry: &LogEntry) -> Vec<DisplayEntry> {
    match entry {
        LogEntry::User { message, timestamp, .. } => parse_user_message(message, *timestamp),
        LogEntry::Assistant { message, timestamp, .. } => parse_assistant_message(message, *timestamp),
        LogEntry::Progress { data, timestamp, .. } => parse_progress_data(data, *timestamp),
        LogEntry::Unknown => Vec::new(),
    }
}
```

No other functions in parser.rs change — `parse_user_message`, `parse_assistant_message`, `parse_progress_data`, `parse_content_blocks`, `parse_content_blocks_vec`, `extract_tool_result_content`, and `merge_tool_results` all remain identical.

### Step 4: Verify

- `cargo check` — confirms type safety
- `cargo clippy` — no new warnings
- `cargo test --lib` — existing tests pass
- `cargo test --test scrolling_tests` — integration tests pass
- Manual smoke test: run the app against real JSONL logs to confirm rendering is unchanged

### Scope boundaries

**Only two files change**: `src/logs/types.rs` and `src/logs/parser.rs`.

**No other files reference LogEntry**: Confirmed via grep — only `types.rs` (definition) and `parser.rs` (deserialization + conversion) use `LogEntry`. The `project.rs` references to `.timestamp` and `.session_id` are on different structs (`TimestampOnly`, `SessionIndexEntry`).

**No behavioral change**: Same JSON is deserialized, same `DisplayEntry` values are produced. The only difference is compile-time type safety replacing runtime string matching.

## Notes
Source commit: 306e27c on ai-slop-refactor. Codebase has evolved so re-planning is required.

