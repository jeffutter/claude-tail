use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<ContentValue>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentValue {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: Option<ToolResultContent>,
        #[serde(default)]
        is_error: Option<bool>,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
}

/// Embedded result for a tool call (merged from a subsequent ToolResult entry)
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub enum DisplayEntry {
    UserMessage {
        text: String,
        timestamp: Option<DateTime<Utc>>,
    },
    AssistantText {
        text: String,
        timestamp: Option<DateTime<Utc>>,
    },
    ToolCall {
        name: String,
        input: String,
        id: String,
        timestamp: Option<DateTime<Utc>>,
        /// Result merged from a following ToolResult entry
        result: Option<ToolCallResult>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        timestamp: Option<DateTime<Utc>>,
    },
    Thinking {
        text: String,
        collapsed: bool,
        timestamp: Option<DateTime<Utc>>,
    },
    HookEvent {
        event: String,
        hook_name: Option<String>,
        command: Option<String>,
        timestamp: Option<DateTime<Utc>>,
    },
    AgentSpawn {
        agent_type: String,
        description: String,
        timestamp: Option<DateTime<Utc>>,
    },
}

impl DisplayEntry {
    pub fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            DisplayEntry::UserMessage { timestamp, .. } => *timestamp,
            DisplayEntry::AssistantText { timestamp, .. } => *timestamp,
            DisplayEntry::ToolCall { timestamp, .. } => *timestamp,
            DisplayEntry::ToolResult { timestamp, .. } => *timestamp,
            DisplayEntry::Thinking { timestamp, .. } => *timestamp,
            DisplayEntry::HookEvent { timestamp, .. } => *timestamp,
            DisplayEntry::AgentSpawn { timestamp, .. } => *timestamp,
        }
    }
}

use std::path::PathBuf;
use std::time::SystemTime;

/// Represents an agent (main or sub-agent) within a session
#[derive(Debug, Clone)]
pub struct Agent {
    /// Agent ID: "main" for the main agent, or the agent ID like "a356e17"
    pub id: String,
    /// Display name: "Main" for main agent, or extracted from filename/type
    pub display_name: String,
    /// Path to the agent's JSONL log file
    pub log_path: PathBuf,
    /// Last modification time of the log file
    pub last_modified: SystemTime,
    /// True for the main agent (pinned at top of list)
    pub is_main: bool,
}

impl Agent {
    /// Returns the timestamp formatted as HH:MM:SS
    pub fn timestamp_str(&self) -> String {
        let datetime: DateTime<Local> = self.last_modified.into();
        datetime.format("%H:%M:%S").to_string()
    }

    /// Returns display string with timestamp: "name (HH:MM:SS)"
    pub fn display_name_with_timestamp(&self) -> String {
        format!("{} ({})", self.display_name, self.timestamp_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use serde_json::json;

    // ── Category 1: LogEntry variant parsing ─────────────────────────────────

    #[test]
    fn test_user_variant_full() {
        let value = json!({
            "type": "user",
            "message": {"role": "user", "content": "hello"},
            "timestamp": "2024-01-01T00:00:00Z",
            "session_id": "sess-abc"
        });
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::User {
                message,
                timestamp,
                session_id,
            } => {
                assert_eq!(message.role.as_deref(), Some("user"));
                assert!(timestamp.is_some());
                assert_eq!(session_id.as_deref(), Some("sess-abc"));
            }
            _ => panic!("expected User variant"),
        }
    }

    #[test]
    fn test_user_variant_minimal() {
        let value = json!({"type": "user", "message": {}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::User {
                message,
                timestamp,
                session_id,
            } => {
                assert!(message.role.is_none());
                assert!(message.content.is_none());
                assert!(timestamp.is_none());
                assert!(session_id.is_none());
            }
            _ => panic!("expected User variant"),
        }
    }

    #[test]
    fn test_assistant_variant_minimal() {
        let value = json!({"type": "assistant", "message": {}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Assistant {
                message,
                timestamp,
                session_id,
            } => {
                assert!(message.role.is_none());
                assert!(message.content.is_none());
                assert!(timestamp.is_none());
                assert!(session_id.is_none());
            }
            _ => panic!("expected Assistant variant"),
        }
    }

    #[test]
    fn test_progress_variant() {
        let value = json!({
            "type": "progress",
            "data": {"nested": {"key": 42, "arr": [1, 2, 3]}}
        });
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Progress { data, .. } => {
                assert_eq!(data["nested"]["key"], 42);
                assert_eq!(data["nested"]["arr"][1], 2);
            }
            _ => panic!("expected Progress variant"),
        }
    }

    #[test]
    fn test_unknown_variant() {
        let value = json!({"type": "completely_unknown", "foo": "bar"});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        assert!(matches!(entry, LogEntry::Unknown));
    }

    // ── Category 2: Nested type parsing ──────────────────────────────────────

    #[test]
    fn test_message_content_text() {
        let value = json!({"type": "user", "message": {"role": "user", "content": "hello world"}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::User { message, .. } => {
                assert_eq!(
                    message.content,
                    Some(ContentValue::Text("hello world".to_string()))
                );
            }
            _ => panic!("expected User variant"),
        }
    }

    #[test]
    fn test_message_content_blocks() {
        let value = json!({
            "type": "assistant",
            "message": {
                "content": [{"type": "text", "text": "hi"}]
            }
        });
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Assistant { message, .. } => match message.content {
                Some(ContentValue::Blocks(blocks)) => {
                    assert_eq!(blocks.len(), 1);
                    assert_eq!(
                        blocks[0],
                        ContentBlock::Text {
                            text: "hi".to_string()
                        }
                    );
                }
                _ => panic!("expected Blocks content"),
            },
            _ => panic!("expected Assistant variant"),
        }
    }

    #[test]
    fn test_content_block_text() {
        let value = json!({"type": "text", "text": "hello"});
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        assert_eq!(
            block,
            ContentBlock::Text {
                text: "hello".to_string()
            }
        );
    }

    #[test]
    fn test_content_block_tool_use() {
        let value = json!({
            "type": "tool_use",
            "id": "tool-123",
            "name": "Bash",
            "input": {"command": "ls"}
        });
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tool-123");
                assert_eq!(name, "Bash");
                assert_eq!(input, json!({"command": "ls"}));
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn test_content_block_tool_result_text() {
        let value = json!({
            "type": "tool_result",
            "tool_use_id": "tool-123",
            "content": "output text"
        });
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tool-123");
                assert_eq!(
                    content,
                    Some(ToolResultContent::Text("output text".to_string()))
                );
                assert!(is_error.is_none());
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_content_block_tool_result_blocks() {
        let value = json!({
            "type": "tool_result",
            "tool_use_id": "tool-abc",
            "content": [{"type": "text", "text": "line1"}]
        });
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::ToolResult { content, .. } => match content {
                Some(ToolResultContent::Blocks(blocks)) => {
                    assert_eq!(blocks.len(), 1);
                    assert_eq!(blocks[0].block_type, "text");
                    assert_eq!(blocks[0].text.as_deref(), Some("line1"));
                }
                _ => panic!("expected Blocks content"),
            },
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_content_block_tool_result_error() {
        let value = json!({
            "type": "tool_result",
            "tool_use_id": "tool-err",
            "content": "error occurred",
            "is_error": true
        });
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tool-err");
                assert_eq!(
                    content,
                    Some(ToolResultContent::Text("error occurred".to_string()))
                );
                assert_eq!(is_error, Some(true));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_content_block_tool_result_explicit_not_error() {
        // is_error: Some(false) is distinct from None — parser uses unwrap_or(false)
        let value = json!({
            "type": "tool_result",
            "tool_use_id": "tool-ok",
            "content": "success",
            "is_error": false
        });
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(is_error, Some(false));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_content_block_thinking() {
        let value = json!({
            "type": "thinking",
            "thinking": "I need to consider this carefully",
            "signature": "sig-xyz"
        });
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "I need to consider this carefully");
                assert_eq!(signature.as_deref(), Some("sig-xyz"));
            }
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn test_content_block_unknown() {
        let value = json!({"type": "image", "source": {"url": "http://example.com/img.png"}});
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        assert_eq!(block, ContentBlock::Unknown);
    }

    // ── Category 3: Optional field handling ──────────────────────────────────

    #[test]
    fn test_optional_timestamp_present() {
        let value = json!({
            "type": "user",
            "message": {},
            "timestamp": "2024-06-15T12:34:56Z"
        });
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::User { timestamp, .. } => {
                let ts = timestamp.expect("timestamp should be Some");
                assert_eq!(ts.year(), 2024);
                assert_eq!(ts.month(), 6);
                assert_eq!(ts.day(), 15);
            }
            _ => panic!("expected User"),
        }
    }

    #[test]
    fn test_optional_timestamp_missing() {
        let value = json!({"type": "user", "message": {}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::User { timestamp, .. } => assert!(timestamp.is_none()),
            _ => panic!("expected User"),
        }
    }

    #[test]
    fn test_optional_session_id_present() {
        let value = json!({
            "type": "assistant",
            "message": {},
            "session_id": "my-session-42"
        });
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Assistant { session_id, .. } => {
                assert_eq!(session_id.as_deref(), Some("my-session-42"));
            }
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn test_optional_session_id_missing() {
        let value = json!({"type": "assistant", "message": {}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Assistant { session_id, .. } => assert!(session_id.is_none()),
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn test_optional_signature_in_thinking_present() {
        let value = json!({"type": "thinking", "thinking": "...", "signature": "abc"});
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::Thinking { signature, .. } => {
                assert_eq!(signature.as_deref(), Some("abc"));
            }
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn test_optional_signature_in_thinking_absent() {
        let value = json!({"type": "thinking", "thinking": "..."});
        let block: ContentBlock = serde_json::from_value(value).unwrap();
        match block {
            ContentBlock::Thinking { signature, .. } => assert!(signature.is_none()),
            _ => panic!("expected Thinking"),
        }
    }

    // ── Category 4: Edge cases ────────────────────────────────────────────────

    #[test]
    fn test_empty_message_content() {
        let value = json!({"type": "user", "message": {}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::User { message, .. } => {
                assert!(message.role.is_none());
                assert!(message.content.is_none());
                assert!(message.model.is_none());
            }
            _ => panic!("expected User"),
        }
    }

    #[test]
    fn test_empty_content_string() {
        let value = json!({"type": "user", "message": {"content": ""}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::User { message, .. } => {
                assert_eq!(message.content, Some(ContentValue::Text("".to_string())));
            }
            _ => panic!("expected User"),
        }
    }

    #[test]
    fn test_empty_blocks_array() {
        let value = json!({"type": "assistant", "message": {"content": []}});
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Assistant { message, .. } => {
                assert_eq!(message.content, Some(ContentValue::Blocks(vec![])));
            }
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn test_mixed_known_unknown_blocks() {
        let value = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "start"},
                    {"type": "image", "source": "somewhere"},
                    {"type": "thinking", "thinking": "hmm"}
                ]
            }
        });
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Assistant { message, .. } => match message.content {
                Some(ContentValue::Blocks(blocks)) => {
                    assert_eq!(blocks.len(), 3);
                    assert_eq!(
                        blocks[0],
                        ContentBlock::Text {
                            text: "start".to_string()
                        }
                    );
                    assert_eq!(blocks[1], ContentBlock::Unknown);
                    assert!(matches!(blocks[2], ContentBlock::Thinking { .. }));
                }
                _ => panic!("expected Blocks"),
            },
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn test_progress_arbitrary_data() {
        let value = json!({
            "type": "progress",
            "data": {
                "a": {"b": {"c": [1, true, null, "x"]}}
            }
        });
        let entry: LogEntry = serde_json::from_value(value).unwrap();
        match entry {
            LogEntry::Progress { data, .. } => {
                assert_eq!(data["a"]["b"]["c"][0], 1);
                assert_eq!(data["a"]["b"]["c"][1], true);
                assert!(data["a"]["b"]["c"][2].is_null());
                assert_eq!(data["a"]["b"]["c"][3], "x");
            }
            _ => panic!("expected Progress"),
        }
    }

    // ── Category 5: Error cases ───────────────────────────────────────────────

    #[test]
    fn test_missing_type_discriminant() {
        // serde(tag = "type") requires the "type" field to be present
        let value = json!({"message": {"role": "user", "content": "hello"}});
        let result = serde_json::from_value::<LogEntry>(value);
        assert!(result.is_err());
    }

    #[test]
    fn test_user_missing_required_message_field() {
        // "message" is non-optional in User variant; omitting it is an error
        let value = json!({"type": "user"});
        let result = serde_json::from_value::<LogEntry>(value);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_json() {
        let result = serde_json::from_str::<LogEntry>("{not valid json}");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_timestamp_format() {
        let value = json!({
            "type": "user",
            "message": {},
            "timestamp": "not-a-timestamp"
        });
        let result = serde_json::from_value::<LogEntry>(value);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_use_missing_id() {
        let value = json!({"type": "tool_use", "name": "Bash", "input": {}});
        let result = serde_json::from_value::<ContentBlock>(value);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_use_missing_name() {
        let value = json!({"type": "tool_use", "id": "x", "input": {}});
        let result = serde_json::from_value::<ContentBlock>(value);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_result_missing_tool_use_id() {
        let value = json!({"type": "tool_result", "content": "output"});
        let result = serde_json::from_value::<ContentBlock>(value);
        assert!(result.is_err());
    }
}
