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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ContentValue {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

    // ===== Variant Deserialization Tests (Happy Path) =====

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
            LogEntry::User {
                message,
                timestamp,
                session_id,
            } => {
                assert_eq!(message.role, Some("user".to_string()));
                assert!(timestamp.is_some());
                assert_eq!(session_id, Some("session-123".to_string()));
            }
            _ => panic!("Expected User variant"),
        }
    }

    #[test]
    fn test_user_variant_minimal() {
        let json = json!({
            "type": "user",
            "message": {}
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::User {
                message,
                timestamp,
                session_id,
            } => {
                assert_eq!(message.role, None);
                assert_eq!(message.content, None);
                assert_eq!(timestamp, None);
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected User variant"),
        }
    }

    #[test]
    fn test_assistant_variant_full() {
        let json = json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": "Hello back",
                "model": "claude-3-opus-20240229"
            },
            "timestamp": "2026-02-06T19:30:01Z"
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Assistant {
                message,
                timestamp,
                session_id,
            } => {
                assert_eq!(message.role, Some("assistant".to_string()));
                assert_eq!(message.model, Some("claude-3-opus-20240229".to_string()));
                assert!(timestamp.is_some());
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected Assistant variant"),
        }
    }

    #[test]
    fn test_assistant_variant_minimal() {
        let json = json!({
            "type": "assistant",
            "message": {}
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Assistant {
                message,
                timestamp,
                session_id,
            } => {
                assert_eq!(message.role, None);
                assert_eq!(message.content, None);
                assert_eq!(timestamp, None);
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected Assistant variant"),
        }
    }

    #[test]
    fn test_progress_variant() {
        let json = json!({
            "type": "progress",
            "data": {
                "step": 1,
                "total": 10,
                "message": "Processing..."
            }
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Progress {
                data,
                timestamp,
                session_id,
            } => {
                assert_eq!(data["step"], 1);
                assert_eq!(data["total"], 10);
                assert_eq!(timestamp, None);
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected Progress variant"),
        }
    }

    #[test]
    fn test_unknown_variant() {
        let json = json!({
            "type": "future_type",
            "some_field": "some_value"
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Unknown => {}
            _ => panic!("Expected Unknown variant"),
        }
    }

    // ===== Nested Type Parsing Tests =====

    #[test]
    fn test_message_content_text() {
        let json = json!({
            "type": "user",
            "message": {
                "content": "Simple text content"
            }
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::User { message, .. } => match message.content {
                Some(ContentValue::Text(text)) => {
                    assert_eq!(text, "Simple text content");
                }
                _ => panic!("Expected Text content"),
            },
            _ => panic!("Expected User variant"),
        }
    }

    #[test]
    fn test_message_content_blocks() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Block 1"},
                    {"type": "text", "text": "Block 2"}
                ]
            }
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Assistant { message, .. } => match message.content {
                Some(ContentValue::Blocks(blocks)) => {
                    assert_eq!(blocks.len(), 2);
                }
                _ => panic!("Expected Blocks content"),
            },
            _ => panic!("Expected Assistant variant"),
        }
    }

    #[test]
    fn test_content_block_text() {
        let json = json!({
            "type": "text",
            "text": "Hello world"
        });

        let block: ContentBlock = serde_json::from_value(json).unwrap();
        match block {
            ContentBlock::Text { text } => {
                assert_eq!(text, "Hello world");
            }
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_content_block_tool_use() {
        let json = json!({
            "type": "tool_use",
            "id": "tool-123",
            "name": "grep",
            "input": {
                "pattern": "error",
                "path": "/var/log"
            }
        });

        let block: ContentBlock = serde_json::from_value(json).unwrap();
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tool-123");
                assert_eq!(name, "grep");
                assert_eq!(input["pattern"], "error");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_content_block_tool_result_text() {
        let json = json!({
            "type": "tool_result",
            "tool_use_id": "tool-123",
            "content": "Result text"
        });

        let block: ContentBlock = serde_json::from_value(json).unwrap();
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tool-123");
                match content {
                    Some(ToolResultContent::Text(text)) => {
                        assert_eq!(text, "Result text");
                    }
                    _ => panic!("Expected text content"),
                }
                assert_eq!(is_error, None);
            }
            _ => panic!("Expected ToolResult block"),
        }
    }

    #[test]
    fn test_content_block_tool_result_blocks() {
        let json = json!({
            "type": "tool_result",
            "tool_use_id": "tool-456",
            "content": [
                {"type": "text", "text": "Line 1"},
                {"type": "text", "text": "Line 2"}
            ]
        });

        let block: ContentBlock = serde_json::from_value(json).unwrap();
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                assert_eq!(tool_use_id, "tool-456");
                match content {
                    Some(ToolResultContent::Blocks(blocks)) => {
                        assert_eq!(blocks.len(), 2);
                        assert_eq!(blocks[0].text, Some("Line 1".to_string()));
                    }
                    _ => panic!("Expected blocks content"),
                }
            }
            _ => panic!("Expected ToolResult block"),
        }
    }

    #[test]
    fn test_content_block_tool_result_error() {
        let json = json!({
            "type": "tool_result",
            "tool_use_id": "tool-789",
            "content": "Error: file not found",
            "is_error": true
        });

        let block: ContentBlock = serde_json::from_value(json).unwrap();
        match block {
            ContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(is_error, Some(true));
            }
            _ => panic!("Expected ToolResult block"),
        }
    }

    #[test]
    fn test_content_block_thinking() {
        let json = json!({
            "type": "thinking",
            "thinking": "Let me consider this...",
            "signature": "sig-abc"
        });

        let block: ContentBlock = serde_json::from_value(json).unwrap();
        match block {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "Let me consider this...");
                assert_eq!(signature, Some("sig-abc".to_string()));
            }
            _ => panic!("Expected Thinking block"),
        }
    }

    #[test]
    fn test_content_block_unknown() {
        let json = json!({
            "type": "future_block_type",
            "some_field": "value"
        });

        let block: ContentBlock = serde_json::from_value(json).unwrap();
        match block {
            ContentBlock::Unknown => {}
            _ => panic!("Expected Unknown block"),
        }
    }

    // ===== Optional Field Handling Tests =====

    #[test]
    fn test_optional_timestamp_present() {
        let json = json!({
            "type": "user",
            "message": {},
            "timestamp": "2026-02-06T19:30:00Z"
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::User { timestamp, .. } => {
                assert!(timestamp.is_some());
                let ts = timestamp.unwrap();
                assert_eq!(ts.year(), 2026);
                assert_eq!(ts.month(), 2);
            }
            _ => panic!("Expected User variant"),
        }
    }

    #[test]
    fn test_optional_timestamp_missing() {
        let json = json!({
            "type": "user",
            "message": {}
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::User { timestamp, .. } => {
                assert_eq!(timestamp, None);
            }
            _ => panic!("Expected User variant"),
        }
    }

    #[test]
    fn test_optional_session_id_present() {
        let json = json!({
            "type": "assistant",
            "message": {},
            "session_id": "sess-xyz"
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Assistant { session_id, .. } => {
                assert_eq!(session_id, Some("sess-xyz".to_string()));
            }
            _ => panic!("Expected Assistant variant"),
        }
    }

    #[test]
    fn test_optional_session_id_missing() {
        let json = json!({
            "type": "assistant",
            "message": {}
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Assistant { session_id, .. } => {
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected Assistant variant"),
        }
    }

    #[test]
    fn test_optional_signature_in_thinking() {
        let json_with_sig = json!({
            "type": "thinking",
            "thinking": "Hmm...",
            "signature": "sig-123"
        });

        let block: ContentBlock = serde_json::from_value(json_with_sig).unwrap();
        match block {
            ContentBlock::Thinking { signature, .. } => {
                assert_eq!(signature, Some("sig-123".to_string()));
            }
            _ => panic!("Expected Thinking block"),
        }

        let json_without_sig = json!({
            "type": "thinking",
            "thinking": "Hmm..."
        });

        let block2: ContentBlock = serde_json::from_value(json_without_sig).unwrap();
        match block2 {
            ContentBlock::Thinking { signature, .. } => {
                assert_eq!(signature, None);
            }
            _ => panic!("Expected Thinking block"),
        }
    }

    // ===== Edge Case Tests =====

    #[test]
    fn test_empty_message_content() {
        let json = json!({
            "type": "user",
            "message": {}
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::User { message, .. } => {
                assert_eq!(message.role, None);
                assert_eq!(message.content, None);
                assert_eq!(message.model, None);
            }
            _ => panic!("Expected User variant"),
        }
    }

    #[test]
    fn test_empty_content_string() {
        let json = json!({
            "type": "user",
            "message": {
                "content": ""
            }
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::User { message, .. } => match message.content {
                Some(ContentValue::Text(text)) => {
                    assert_eq!(text, "");
                }
                _ => panic!("Expected Text content"),
            },
            _ => panic!("Expected User variant"),
        }
    }

    #[test]
    fn test_empty_blocks_array() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": []
            }
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Assistant { message, .. } => match message.content {
                Some(ContentValue::Blocks(blocks)) => {
                    assert_eq!(blocks.len(), 0);
                }
                _ => panic!("Expected Blocks content"),
            },
            _ => panic!("Expected Assistant variant"),
        }
    }

    #[test]
    fn test_mixed_known_unknown_blocks() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Known text block"},
                    {"type": "future_type", "data": "unknown"},
                    {"type": "thinking", "thinking": "Known thinking block"}
                ]
            }
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Assistant { message, .. } => match message.content {
                Some(ContentValue::Blocks(blocks)) => {
                    assert_eq!(blocks.len(), 3);
                    // First block is Text
                    match &blocks[0] {
                        ContentBlock::Text { text } => {
                            assert_eq!(text, "Known text block");
                        }
                        _ => panic!("Expected Text block at index 0"),
                    }
                    // Second block is Unknown
                    match &blocks[1] {
                        ContentBlock::Unknown => {}
                        _ => panic!("Expected Unknown block at index 1"),
                    }
                    // Third block is Thinking
                    match &blocks[2] {
                        ContentBlock::Thinking { thinking, .. } => {
                            assert_eq!(thinking, "Known thinking block");
                        }
                        _ => panic!("Expected Thinking block at index 2"),
                    }
                }
                _ => panic!("Expected Blocks content"),
            },
            _ => panic!("Expected Assistant variant"),
        }
    }

    #[test]
    fn test_progress_arbitrary_data() {
        let json = json!({
            "type": "progress",
            "data": {
                "nested": {
                    "deeply": {
                        "structured": [1, 2, 3]
                    }
                },
                "other_field": "value"
            }
        });

        let entry: LogEntry = serde_json::from_value(json).unwrap();
        match entry {
            LogEntry::Progress { data, .. } => {
                assert!(data["nested"]["deeply"]["structured"].is_array());
                assert_eq!(data["other_field"], "value");
            }
            _ => panic!("Expected Progress variant"),
        }
    }

    // ===== Error Case Tests =====

    #[test]
    fn test_missing_type_field() {
        let json = json!({
            "message": {}
        });

        let result: Result<LogEntry, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_json() {
        let invalid_json = r#"{not valid json}"#;
        let result: Result<LogEntry, _> = serde_json::from_str(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_timestamp_format() {
        let json = json!({
            "type": "user",
            "message": {},
            "timestamp": "not-a-date"
        });

        let result: Result<LogEntry, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_use_missing_id() {
        let json = json!({
            "type": "tool_use",
            "name": "grep",
            "input": {}
        });

        let result: Result<ContentBlock, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_use_missing_name() {
        let json = json!({
            "type": "tool_use",
            "id": "tool-123",
            "input": {}
        });

        let result: Result<ContentBlock, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_result_missing_tool_use_id() {
        let json = json!({
            "type": "tool_result",
            "content": "result"
        });

        let result: Result<ContentBlock, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }
}
