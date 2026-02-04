use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<ContentValue>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentValue {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn display_with_timestamp(&self) -> String {
        format!("{} ({})", self.display_name, self.timestamp_str())
    }
}
