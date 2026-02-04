use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::types::{
    ContentBlock, ContentValue, DisplayEntry, LogEntry, ToolCallResult, ToolResultContent,
};

pub fn parse_jsonl_file(path: &Path) -> Result<Vec<DisplayEntry>> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<LogEntry>(&line) {
            Ok(entry) => {
                entries.extend(convert_log_entry(&entry));
            }
            Err(_) => {
                // Skip malformed lines - error recovery
                continue;
            }
        }
    }

    Ok(entries)
}

pub fn parse_jsonl_from_position(path: &Path, position: u64) -> Result<(Vec<DisplayEntry>, u64)> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(position))?;

    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let new_position = position + content.len() as u64;
    let mut entries = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<LogEntry>(line) {
            Ok(entry) => {
                entries.extend(convert_log_entry(&entry));
            }
            Err(_) => continue,
        }
    }

    Ok((entries, new_position))
}

fn convert_log_entry(entry: &LogEntry) -> Vec<DisplayEntry> {
    let mut display_entries = Vec::new();
    let timestamp = entry.timestamp;

    match entry.entry_type.as_str() {
        "user" => {
            if let Some(ref message) = entry.message {
                display_entries.extend(parse_user_message(message, timestamp));
            }
        }
        "progress" => {
            if let Some(ref data) = entry.data {
                display_entries.extend(parse_progress_data(data, timestamp));
            }
        }
        "assistant" => {
            if let Some(ref message) = entry.message {
                display_entries.extend(parse_assistant_message(message, timestamp));
            }
        }
        _ => {}
    }

    display_entries
}

fn parse_user_message(
    message: &super::types::MessageContent,
    timestamp: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<DisplayEntry> {
    let mut entries = Vec::new();

    match &message.content {
        Some(ContentValue::Text(text)) => {
            if !text.is_empty() {
                entries.push(DisplayEntry::UserMessage {
                    text: text.clone(),
                    timestamp,
                });
            }
        }
        Some(ContentValue::Blocks(blocks)) => {
            let mut text_parts = Vec::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text } => {
                        text_parts.push(text.as_str());
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        // Flush any accumulated text first
                        if !text_parts.is_empty() {
                            entries.push(DisplayEntry::UserMessage {
                                text: text_parts.join("\n"),
                                timestamp,
                            });
                            text_parts.clear();
                        }
                        // Add the tool result
                        let content_str = match content {
                            Some(ToolResultContent::Text(text)) => text.clone(),
                            Some(ToolResultContent::Blocks(blocks)) => blocks
                                .iter()
                                .filter_map(|b| b.text.as_ref())
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n"),
                            None => String::new(),
                        };
                        entries.push(DisplayEntry::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: content_str,
                            is_error: is_error.unwrap_or(false),
                            timestamp,
                        });
                    }
                    _ => {}
                }
            }
            // Flush remaining text
            if !text_parts.is_empty() {
                entries.push(DisplayEntry::UserMessage {
                    text: text_parts.join("\n"),
                    timestamp,
                });
            }
        }
        None => {}
    }

    entries
}

fn parse_progress_data(
    data: &serde_json::Value,
    timestamp: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<DisplayEntry> {
    let mut entries = Vec::new();

    // Check for message in progress data (assistant responses and tool results come through here)
    if let Some(message) = data.get("message") {
        if let Some(role) = message.get("role").and_then(|r| r.as_str()) {
            if let Some(content) = message.get("content") {
                match role {
                    "assistant" => {
                        entries.extend(parse_content_blocks(content, timestamp));
                    }
                    "user" => {
                        // Tool results come as user messages
                        entries.extend(parse_content_blocks(content, timestamp));
                    }
                    _ => {}
                }
            }
        }
    }

    // Check for hook events
    if let Some(hook_event) = data.get("hookEvent").and_then(|h| h.as_str()) {
        let hook_name = data
            .get("hookName")
            .and_then(|h| h.as_str())
            .map(|s| s.to_string());
        let command = data
            .get("command")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
        entries.push(DisplayEntry::HookEvent {
            event: hook_event.to_string(),
            hook_name,
            command,
            timestamp,
        });
    }

    // Check for agent spawns
    if let Some(agent_type) = data.get("agentType").and_then(|a| a.as_str()) {
        let description = data
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        entries.push(DisplayEntry::AgentSpawn {
            agent_type: agent_type.to_string(),
            description,
            timestamp,
        });
    }

    entries
}

fn parse_assistant_message(
    message: &super::types::MessageContent,
    timestamp: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<DisplayEntry> {
    match &message.content {
        Some(ContentValue::Text(text)) => {
            vec![DisplayEntry::AssistantText {
                text: text.clone(),
                timestamp,
            }]
        }
        Some(ContentValue::Blocks(blocks)) => parse_content_blocks_vec(blocks, timestamp),
        None => Vec::new(),
    }
}

fn parse_content_blocks(
    content: &serde_json::Value,
    timestamp: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<DisplayEntry> {
    let mut entries = Vec::new();

    if let Some(blocks) = content.as_array() {
        for block in blocks {
            if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            entries.push(DisplayEntry::AssistantText {
                                text: text.to_string(),
                                timestamp,
                            });
                        }
                    }
                    "tool_use" => {
                        let name = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let id = block
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = block
                            .get("input")
                            .map(|i| serde_json::to_string_pretty(i).unwrap_or_default())
                            .unwrap_or_default();
                        entries.push(DisplayEntry::ToolCall {
                            name,
                            input,
                            id,
                            timestamp,
                            result: None,
                        });
                    }
                    "tool_result" => {
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("")
                            .to_string();
                        let is_error = block
                            .get("is_error")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);
                        let content = extract_tool_result_content(block.get("content"));
                        entries.push(DisplayEntry::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                            timestamp,
                        });
                    }
                    "thinking" => {
                        if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                            entries.push(DisplayEntry::Thinking {
                                text: thinking.to_string(),
                                collapsed: true,
                                timestamp,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    entries
}

fn parse_content_blocks_vec(
    blocks: &[ContentBlock],
    timestamp: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<DisplayEntry> {
    let mut entries = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                entries.push(DisplayEntry::AssistantText {
                    text: text.clone(),
                    timestamp,
                });
            }
            ContentBlock::ToolUse { id, name, input } => {
                entries.push(DisplayEntry::ToolCall {
                    name: name.clone(),
                    input: serde_json::to_string_pretty(input).unwrap_or_default(),
                    id: id.clone(),
                    timestamp,
                    result: None,
                });
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let content_str = match content {
                    Some(ToolResultContent::Text(text)) => text.clone(),
                    Some(ToolResultContent::Blocks(blocks)) => blocks
                        .iter()
                        .filter_map(|b| b.text.as_ref())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                    None => String::new(),
                };
                entries.push(DisplayEntry::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content_str,
                    is_error: is_error.unwrap_or(false),
                    timestamp,
                });
            }
            ContentBlock::Thinking { thinking, .. } => {
                entries.push(DisplayEntry::Thinking {
                    text: thinking.clone(),
                    collapsed: true,
                    timestamp,
                });
            }
            ContentBlock::Unknown => {}
        }
    }

    entries
}

fn extract_tool_result_content(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Merge ToolResult entries into their preceding ToolCall entries when they match by ID.
/// This creates a cleaner display where results appear inline with their commands.
/// Results that don't immediately follow their call are kept separate.
pub fn merge_tool_results(entries: Vec<DisplayEntry>) -> Vec<DisplayEntry> {
    let mut result = Vec::with_capacity(entries.len());
    let mut skip_next = false;

    for (i, entry) in entries.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        match entry {
            DisplayEntry::ToolCall {
                id,
                name,
                input,
                timestamp,
                result: _,
            } => {
                // Look ahead for a matching ToolResult
                let merged_result = entries.get(i + 1).and_then(|next| {
                    if let DisplayEntry::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    } = next
                    {
                        if tool_use_id == id {
                            skip_next = true;
                            Some(ToolCallResult {
                                content: content.clone(),
                                is_error: *is_error,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                result.push(DisplayEntry::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                    timestamp: *timestamp,
                    result: merged_result,
                });
            }
            _ => {
                result.push(entry.clone());
            }
        }
    }

    result
}
