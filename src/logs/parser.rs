use anyhow::Result;
use serde_json::error::Category;
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::types::{
    ContentBlock, ContentValue, DisplayEntry, LogEntry, ToolCallResult, ToolResultContent,
};

/// Result of parsing a JSONL file, including any errors encountered
pub struct ParseResult {
    pub entries: Vec<DisplayEntry>,
    /// Parse errors (line descriptions, not fatal)
    pub errors: Vec<String>,
    /// Number of bytes read from the file
    pub bytes_read: u64,
}

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

                // Skip trailing whitespace (including CRLF)
                let whitespace_len = content[current_pos..]
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
                            base_position + error_offset as u64,
                            e
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

/// Async version of parse_jsonl_file that runs parsing on a background thread
pub async fn parse_jsonl_file_async(path: PathBuf) -> Result<ParseResult> {
    tokio::task::spawn_blocking(move || parse_jsonl_file(&path))
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
}

/// Async version of parse_jsonl_from_position that runs parsing on a background thread
pub async fn parse_jsonl_from_position_async(path: PathBuf, position: u64) -> Result<ParseResult> {
    tokio::task::spawn_blocking(move || parse_jsonl_from_position(&path, position))
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
}

fn convert_log_entry(entry: &LogEntry) -> Vec<DisplayEntry> {
    match entry {
        LogEntry::User {
            message, timestamp, ..
        } => parse_user_message(message, *timestamp),
        LogEntry::Assistant {
            message, timestamp, ..
        } => parse_assistant_message(message, *timestamp),
        LogEntry::Progress {
            data, timestamp, ..
        } => parse_progress_data(data, *timestamp),
        LogEntry::Unknown => Vec::new(),
    }
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
    if let Some(message) = data.get("message")
        && let Some(role) = message.get("role").and_then(|r| r.as_str())
        && let Some(content) = message.get("content")
    {
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
pub fn merge_tool_results(mut entries: Vec<DisplayEntry>) -> Vec<DisplayEntry> {
    let mut result = Vec::with_capacity(entries.len());
    let mut iter = entries.drain(..);
    let mut next_entry = iter.next();

    while let Some(entry) = next_entry.take() {
        match entry {
            DisplayEntry::ToolCall {
                id,
                name,
                input,
                timestamp,
                result: _,
            } => {
                // Peek at next entry for a matching ToolResult
                next_entry = iter.next();
                let merged_result = if let Some(DisplayEntry::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                }) = &next_entry
                {
                    if tool_use_id == &id {
                        // Consume the result entry by extracting its data
                        // (content must be cloned since we're peeking at next_entry)
                        let consumed_result = Some(ToolCallResult {
                            content: content.clone(),
                            is_error: *is_error,
                        });
                        next_entry = iter.next(); // Skip the consumed result
                        consumed_result
                    } else {
                        None
                    }
                } else {
                    None
                };

                result.push(DisplayEntry::ToolCall {
                    id,
                    name,
                    input,
                    timestamp,
                    result: merged_result,
                });
            }
            other => {
                result.push(other);
                next_entry = iter.next();
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn user_entry(text: &str) -> String {
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": text}
        })
        .to_string()
    }

    fn assistant_entry(text: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": text}
        })
        .to_string()
    }

    fn progress_entry() -> String {
        serde_json::json!({
            "type": "progress",
            "data": {"message": {"role": "assistant", "content": [{"type": "text", "text": "thinking..."}]}}
        })
        .to_string()
    }

    // ── Category 1: Byte position tracking ───────────────────────────────────

    #[test]
    fn test_single_entry_position() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("hello");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.bytes_read, (line.len() + 1) as u64);
    }

    #[test]
    fn test_multiple_entries_position() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = assistant_entry("second");
        let line3 = user_entry("third");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();

        let expected_bytes = (line1.len() + 1 + line2.len() + 1 + line3.len() + 1) as u64;
        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.bytes_read, expected_bytes);
    }

    #[test]
    fn test_empty_lines_position() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        // Empty line between entries
        writeln!(file, "{}", line1).unwrap();
        writeln!(file).unwrap();
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 2);
        // Position should include the empty line
        let expected = (line1.len() + 1 + 1 + line2.len() + 1) as u64;
        assert_eq!(result.bytes_read, expected);
    }

    #[test]
    fn test_trailing_newline_position() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        // Trailing newline is included
        assert_eq!(result.bytes_read, (line.len() + 1) as u64);
    }

    #[test]
    fn test_no_trailing_newline_position() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        write!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 1);
        // Complete JSON without newline is still consumed
        assert_eq!(result.bytes_read, line.len() as u64);
    }

    #[test]
    fn test_whitespace_only_lines_position() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "   \t  ").unwrap(); // whitespace-only line
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 2);
        let expected = (line1.len() + 1 + 7 + line2.len() + 1) as u64;
        assert_eq!(result.bytes_read, expected);
    }

    // ── Category 2: Resume from position ─────────────────────────────────────

    #[test]
    fn test_resume_from_start() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result_full = parse_jsonl_file(file.path()).unwrap();
        let result_from_zero = parse_jsonl_from_position(file.path(), 0).unwrap();

        assert_eq!(result_full.entries.len(), result_from_zero.entries.len());
        assert_eq!(result_full.bytes_read, result_from_zero.bytes_read);
    }

    #[test]
    fn test_resume_from_middle() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let first_entry_end = (line1.len() + 1) as u64;
        let result = parse_jsonl_from_position(file.path(), first_entry_end).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert!(
            matches!(&result.entries[0], DisplayEntry::UserMessage { text, .. } if text == "second")
        );
    }

    #[test]
    fn test_resume_from_end() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("only");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let end_pos = (line.len() + 1) as u64;
        let result = parse_jsonl_from_position(file.path(), end_pos).unwrap();
        assert_eq!(result.entries.len(), 0);
        assert_eq!(result.bytes_read, end_pos);
    }

    #[test]
    fn test_incremental_append() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result1.entries.len(), 1);
        let pos = result1.bytes_read;

        // Append second entry
        let line2 = assistant_entry("second");
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_from_position(file.path(), pos).unwrap();
        assert_eq!(result2.entries.len(), 1);
        assert!(
            matches!(&result2.entries[0], DisplayEntry::AssistantText { text, .. } if text == "second")
        );
    }

    #[test]
    fn test_resume_multiple_increments() {
        let mut file = NamedTempFile::new().unwrap();

        // Round 1
        let line1 = user_entry("round1");
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();
        let result1 = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result1.entries.len(), 1);
        let pos1 = result1.bytes_read;

        // Round 2
        let line2 = assistant_entry("round2");
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();
        let result2 = parse_jsonl_from_position(file.path(), pos1).unwrap();
        assert_eq!(result2.entries.len(), 1);
        let pos2 = result2.bytes_read;

        // Round 3
        let line3 = user_entry("round3");
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();
        let result3 = parse_jsonl_from_position(file.path(), pos2).unwrap();
        assert_eq!(result3.entries.len(), 1);
        assert!(
            matches!(&result3.entries[0], DisplayEntry::UserMessage { text, .. } if text == "round3")
        );
    }

    // ── Category 3: Partial / incomplete lines at EOF ─────────────────────────

    #[test]
    fn test_truncated_json_at_eof() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{{\"type\":\"us").unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 0);
        assert_eq!(result.errors.len(), 0);
        assert_eq!(result.bytes_read, 0);
    }

    #[test]
    fn test_complete_then_truncated() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("complete");
        writeln!(file, "{}", line1).unwrap();
        write!(file, "{{\"type\":\"us").unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.errors.len(), 0);
        // Position is after the complete entry, before the truncated one
        assert_eq!(result.bytes_read, (line1.len() + 1) as u64);
    }

    #[test]
    fn test_incomplete_append_then_complete() {
        let mut file = NamedTempFile::new().unwrap();
        // Write partial entry
        write!(file, "{{\"type\":\"us").unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result1.entries.len(), 0);
        assert_eq!(result1.errors.len(), 0);
        // Position stays at 0 (incomplete JSON not consumed)
        assert_eq!(result1.bytes_read, 0);

        // Complete the entry
        let remainder = r#"er", "message": {"role": "user", "content": "done"}}"#;
        writeln!(file, "{}", remainder).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_from_position(file.path(), result1.bytes_read).unwrap();
        assert_eq!(result2.entries.len(), 1);
    }

    #[test]
    fn test_partial_line_no_newline() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("no newline at end");
        write!(file, "{}", line).unwrap(); // no trailing \n
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        // Complete JSON is parsed even without trailing newline
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.bytes_read, line.len() as u64);
    }

    #[test]
    fn test_mid_stream_growth() {
        let mut file = NamedTempFile::new().unwrap();
        let mut pos = 0u64;
        let mut total_entries = 0;

        for i in 0..5 {
            let line = user_entry(&format!("message {}", i));
            writeln!(file, "{}", line).unwrap();
            file.flush().unwrap();

            let result = parse_jsonl_from_position(file.path(), pos).unwrap();
            assert_eq!(result.entries.len(), 1);
            pos = result.bytes_read;
            total_entries += 1;
        }
        assert_eq!(total_entries, 5);
    }

    // ── Category 4: Error recovery ────────────────────────────────────────────

    #[test]
    fn test_single_malformed_line() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "not valid json at all").unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 0);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_malformed_between_valid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", user_entry("before")).unwrap();
        writeln!(file, "{{bad json}}").unwrap();
        writeln!(file, "{}", user_entry("after")).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_multiple_malformed_lines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", user_entry("good1")).unwrap();
        writeln!(file, "bad1").unwrap();
        writeln!(file, "bad2").unwrap();
        writeln!(file, "{}", user_entry("good2")).unwrap();
        writeln!(file, "bad3").unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.errors.len(), 3);
    }

    #[test]
    fn test_errors_contain_byte_offsets() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "bad json here").unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].contains("Parse error at byte"),
            "Error message '{}' should contain 'Parse error at byte'",
            result.errors[0]
        );
        // Verify the offset is small (near byte 0) for a bad line at the start of the file
        assert!(
            result.errors[0].contains("byte 0"),
            "Error message '{}' should report offset near byte 0 for first-line error",
            result.errors[0]
        );
    }

    #[test]
    fn test_errors_dont_stop_parsing() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "bad json early").unwrap();
        for i in 0..10 {
            writeln!(file, "{}", user_entry(&format!("msg {}", i))).unwrap();
        }
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 10);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_empty_lines_not_errors() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", user_entry("first")).unwrap();
        writeln!(file).unwrap(); // empty line
        writeln!(file, "   ").unwrap(); // whitespace-only line
        writeln!(file, "{}", user_entry("second")).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_unknown_type_not_error() {
        let mut file = NamedTempFile::new().unwrap();
        let unknown = serde_json::json!({"type": "future_type", "data": {}}).to_string();
        writeln!(file, "{}", unknown).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        // Unknown type produces 0 DisplayEntries but is not an error
        assert_eq!(result.entries.len(), 0);
        assert_eq!(result.errors.len(), 0);
    }

    // ── Category 5: Edge cases ────────────────────────────────────────────────

    #[test]
    fn test_utf8_multibyte_characters() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("Hello 日本語 🌟 café");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.errors.len(), 0);
        assert_eq!(result.bytes_read, (line.len() + 1) as u64);
    }

    #[test]
    fn test_crlf_line_endings() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        write!(file, "{}\r\n", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.errors.len(), 0);
        // Position includes both \r and \n bytes
        assert_eq!(result.bytes_read, (line.len() + 2) as u64);
    }

    #[test]
    fn test_very_long_line() {
        let mut file = NamedTempFile::new().unwrap();
        let big_text = "x".repeat(15_000);
        let line = user_entry(&big_text);
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.errors.len(), 0);
    }

    #[test]
    fn test_empty_file() {
        let file = NamedTempFile::new().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result.entries.len(), 0);
        assert_eq!(result.errors.len(), 0);
        assert_eq!(result.bytes_read, 0);
    }

    #[test]
    fn test_mixed_valid_unknown_types() {
        let mut file = NamedTempFile::new().unwrap();
        let unknown = serde_json::json!({"type": "future_type", "data": {}}).to_string();
        writeln!(file, "{}", user_entry("before")).unwrap();
        writeln!(file, "{}", unknown).unwrap();
        writeln!(file, "{}", assistant_entry("after")).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        // user + assistant = 2 display entries; unknown produces 0
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.errors.len(), 0);
    }

    // ── Bonus: merge_tool_results ─────────────────────────────────────────────

    #[test]
    fn test_merge_tool_results_matching_ids() {
        let entries = vec![
            DisplayEntry::ToolCall {
                name: "Bash".to_string(),
                input: "{}".to_string(),
                id: "tool-1".to_string(),
                timestamp: None,
                result: None,
            },
            DisplayEntry::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: "output".to_string(),
                is_error: false,
                timestamp: None,
            },
        ];
        let merged = merge_tool_results(entries);
        assert_eq!(merged.len(), 1);
        match &merged[0] {
            DisplayEntry::ToolCall { result, .. } => {
                assert!(result.is_some());
                assert_eq!(result.as_ref().unwrap().content, "output");
                assert!(!result.as_ref().unwrap().is_error);
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_merge_tool_results_non_matching_ids() {
        let entries = vec![
            DisplayEntry::ToolCall {
                name: "Bash".to_string(),
                input: "{}".to_string(),
                id: "tool-1".to_string(),
                timestamp: None,
                result: None,
            },
            DisplayEntry::ToolResult {
                tool_use_id: "tool-OTHER".to_string(),
                content: "output".to_string(),
                is_error: false,
                timestamp: None,
            },
        ];
        let merged = merge_tool_results(entries);
        // Non-matching: both entries kept separate
        assert_eq!(merged.len(), 2);
        match &merged[0] {
            DisplayEntry::ToolCall { result, .. } => assert!(result.is_none()),
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_progress_entry_yields_assistant_text() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", progress_entry()).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();
        // Progress entries with assistant text content become AssistantText
        assert!(!result.entries.is_empty());
        assert!(matches!(
            result.entries[0],
            DisplayEntry::AssistantText { .. }
        ));
    }

    #[test]
    fn test_merge_tool_results_intervening_entry() {
        // A ToolResult that doesn't immediately follow its ToolCall is NOT merged
        let entries = vec![
            DisplayEntry::ToolCall {
                name: "Bash".to_string(),
                input: "{}".to_string(),
                id: "tool-1".to_string(),
                timestamp: None,
                result: None,
            },
            DisplayEntry::AssistantText {
                text: "thinking...".to_string(),
                timestamp: None,
            },
            DisplayEntry::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: "output".to_string(),
                is_error: false,
                timestamp: None,
            },
        ];
        let merged = merge_tool_results(entries);
        // All three entries kept separate; ToolCall has no merged result
        assert_eq!(merged.len(), 3);
        match &merged[0] {
            DisplayEntry::ToolCall { result, .. } => assert!(result.is_none()),
            _ => panic!("expected ToolCall"),
        }
    }
}
