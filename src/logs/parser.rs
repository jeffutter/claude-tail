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

/// Parse JSONL content using StreamDeserializer
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
                // Successfully parsed an entry
                entries.extend(convert_log_entry(&entry));
                let offset = stream.byte_offset();
                current_pos += offset;

                // Skip trailing whitespace (including newlines) to match line-based behavior
                let remaining = &content[current_pos..];
                let whitespace_len = remaining
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .map(|c| c.len_utf8())
                    .sum::<usize>();

                current_pos += whitespace_len;
                last_valid_position = current_pos;
            }
            Some(Err(e)) => {
                // Error occurred during parsing
                let error_offset = current_pos + stream.byte_offset();

                match e.classify() {
                    Category::Eof => {
                        // Incomplete JSON at EOF - don't advance position
                        // This allows re-reading when more data is written
                        break;
                    }
                    Category::Syntax | Category::Data => {
                        // Malformed JSON - record error and try to skip to next line
                        errors.push(format!(
                            "Parse error at byte {}: {}",
                            base_position + error_offset as u64,
                            e
                        ));

                        // Try to recover by skipping to the next newline
                        if let Some(remaining) = slice.get(stream.byte_offset()..) {
                            if let Some(newline_pos) = remaining.find('\n') {
                                // Found newline - advance past it
                                current_pos = current_pos + stream.byte_offset() + newline_pos + 1;
                                last_valid_position = current_pos;
                            } else {
                                // No newline found - we're at EOF with malformed data
                                last_valid_position = content.len();
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    Category::Io => {
                        // I/O error (shouldn't happen with string deserialization)
                        return Err(anyhow::anyhow!("I/O error during deserialization: {}", e));
                    }
                }
            }
            None => {
                // End of stream
                break;
            }
        }
    }

    Ok(ParseResult {
        entries,
        errors,
        bytes_read: base_position + last_valid_position as u64,
    })
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

pub(super) fn convert_log_entry(entry: &LogEntry) -> Vec<DisplayEntry> {
    match entry {
        LogEntry::User {
            message, timestamp, ..
        } => parse_user_message(message, *timestamp),
        LogEntry::Progress {
            data, timestamp, ..
        } => parse_progress_data(data, *timestamp),
        LogEntry::Assistant {
            message, timestamp, ..
        } => parse_assistant_message(message, *timestamp),
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

    // Helper functions to create valid JSONL entries
    fn user_entry(text: &str) -> String {
        serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": text
            }
        })
        .to_string()
    }

    fn assistant_entry(text: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": text
            }
        })
        .to_string()
    }

    // ============================================================================
    // 1. Byte Position Tracking Tests
    // ============================================================================

    #[test]
    fn test_position_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 0);
        assert!(result.errors.is_empty());
        assert_eq!(result.bytes_read, 0);
    }

    #[test]
    fn test_position_single_line() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("hello");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        // line + newline = bytes consumed
        assert_eq!(result.bytes_read, (line.len() + 1) as u64);
    }

    #[test]
    fn test_position_multiple_lines() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        let line3 = user_entry("third");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 3);
        assert!(result.errors.is_empty());
        let expected_bytes = (line1.len() + 1 + line2.len() + 1 + line3.len() + 1) as u64;
        assert_eq!(result.bytes_read, expected_bytes);
    }

    #[test]
    fn test_position_no_trailing_newline() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("hello");
        write!(file, "{}", line).unwrap(); // No newline
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        // The parser treats EOF as a complete line, so this WILL be parsed
        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        // Position advances to end of file (no newline)
        assert_eq!(result.bytes_read, line.len() as u64);
    }

    #[test]
    fn test_position_with_utf8() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("hello 😀");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        // Verify position accounts for multi-byte UTF-8
        assert_eq!(result.bytes_read, (line.len() + 1) as u64);
    }

    #[test]
    fn test_position_accumulates_correctly() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_file(file.path()).unwrap();
        let pos1 = result1.bytes_read;

        // Append a second line
        let line2 = user_entry("second");
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_from_position(file.path(), pos1).unwrap();
        let pos2 = result2.bytes_read;

        // Position should be absolute
        assert_eq!(pos2, pos1 + (line2.len() + 1) as u64);
    }

    // ============================================================================
    // 2. Resuming from Position Tests
    // ============================================================================

    #[test]
    fn test_resume_from_zero() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_file(file.path()).unwrap();
        let result2 = parse_jsonl_from_position(file.path(), 0).unwrap();

        assert_eq!(result1.entries.len(), result2.entries.len());
        assert_eq!(result1.bytes_read, result2.bytes_read);
    }

    #[test]
    fn test_resume_from_middle() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");

        // Write first line, parse, get position
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();
        let result1 = parse_jsonl_file(file.path()).unwrap();
        let pos = result1.bytes_read;

        // Append second line
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        // Resume from saved position
        let result2 = parse_jsonl_from_position(file.path(), pos).unwrap();

        assert_eq!(result2.entries.len(), 1); // Only the new line
        assert_eq!(result2.bytes_read, pos + (line2.len() + 1) as u64);
    }

    #[test]
    fn test_resume_position_beyond_eof() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        // Seek beyond EOF
        let file_len = std::fs::metadata(file.path()).unwrap().len();
        let result = parse_jsonl_from_position(file.path(), file_len + 1000).unwrap();

        assert!(result.entries.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_resume_incremental_accumulation() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        let line3 = user_entry("third");

        // First append and parse
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();
        let result1 = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result1.entries.len(), 1);
        let pos1 = result1.bytes_read;

        // Second append and parse
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();
        let result2 = parse_jsonl_from_position(file.path(), pos1).unwrap();
        assert_eq!(result2.entries.len(), 1);
        let pos2 = result2.bytes_read;

        // Third append and parse
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();
        let result3 = parse_jsonl_from_position(file.path(), pos2).unwrap();
        assert_eq!(result3.entries.len(), 1);
    }

    #[test]
    fn test_resume_preserves_position_on_empty_read() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_file(file.path()).unwrap();
        let pos = result1.bytes_read;

        // Parse again from EOF without new data
        let result2 = parse_jsonl_from_position(file.path(), pos).unwrap();

        assert!(result2.entries.is_empty());
        assert_eq!(result2.bytes_read, pos);
    }

    // ============================================================================
    // 3. Partial/Incomplete Lines at EOF Tests
    // ============================================================================

    #[test]
    fn test_incomplete_json_not_consumed() {
        let mut file = NamedTempFile::new().unwrap();
        // Write incomplete JSON (no closing brace, no newline)
        let incomplete = br#"{"type":"user","message":{"#;
        file.write_all(incomplete).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert!(result.entries.is_empty());
        // StreamDeserializer correctly detects EOF (incomplete JSON) - no errors recorded
        assert!(result.errors.is_empty());
        // Position does NOT advance - we'll retry when more data is written
        assert_eq!(result.bytes_read, 0);
    }

    #[test]
    fn test_incomplete_then_complete() {
        let mut file = NamedTempFile::new().unwrap();
        // Start with a complete, parseable first line
        let line1 = user_entry("first");
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result1.entries.len(), 1);

        // Now write an incomplete line (no newline)
        file.write_all(br#"{"type":"user","message":{"#).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_file(file.path()).unwrap();
        // Still only 1 entry (the first one), incomplete JSON at EOF has no error
        assert_eq!(result2.entries.len(), 1);
        assert_eq!(result2.errors.len(), 0);

        // Complete the incomplete line
        file.write_all(b"\"role\":\"user\",\"content\":\"hello\"}}\n")
            .unwrap();
        file.flush().unwrap();

        let result3 = parse_jsonl_file(file.path()).unwrap();
        // Now both lines should parse successfully
        assert_eq!(result3.entries.len(), 2);
        assert_eq!(result3.errors.len(), 0);
    }

    #[test]
    fn test_complete_json_no_newline() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        write!(file, "{}", line).unwrap(); // No newline
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        // Parser treats EOF as complete line, so this WILL be parsed
        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.bytes_read, line.len() as u64);
    }

    #[test]
    fn test_multiple_lines_last_incomplete() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        file.write_all(br#"{"partial":"#).unwrap(); // Incomplete line
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 2); // First 2 parsed
        assert_eq!(result.errors.len(), 0); // Incomplete JSON at EOF - no error (EOF condition)
        // Position does NOT include incomplete line - will retry when complete
        let expected_pos = (line1.len() + 1 + line2.len() + 1) as u64;
        assert_eq!(result.bytes_read, expected_pos);
    }

    #[test]
    fn test_complete_line_followed_by_incomplete() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("complete");
        writeln!(file, "{}", line1).unwrap();
        file.write_all(br#"{"partial"#).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.errors.len(), 0); // Incomplete JSON at EOF - no error (EOF condition)
        // Position does NOT include incomplete line - will retry when complete
        assert_eq!(result.bytes_read, (line1.len() + 1) as u64);
    }

    // ============================================================================
    // 4. Error Recovery Tests
    // ============================================================================

    #[test]
    fn test_malformed_json_complete_line() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"{\"invalid\": json}\n").unwrap(); // Invalid JSON
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert!(result.entries.is_empty());
        assert_eq!(result.errors.len(), 1); // Error recorded
        assert!(result.errors[0].contains("Parse error at byte"));
        assert!(result.bytes_read > 0); // Position advanced past line
    }

    #[test]
    fn test_malformed_json_incomplete_line() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"{\"invalid\": json}").unwrap(); // Invalid + no newline
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert!(result.entries.is_empty());
        // Parser treats EOF as complete, so error IS recorded
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.bytes_read, 17); // Advances to EOF
    }

    #[test]
    fn test_errors_dont_stop_parsing() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("valid");
        writeln!(file, "{}", line1).unwrap();
        file.write_all(b"{\"invalid\": json}\n").unwrap(); // Invalid
        let line3 = assistant_entry("also valid");
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 2); // 2 valid entries
        assert_eq!(result.errors.len(), 1); // 1 error recorded
    }

    #[test]
    fn test_empty_lines_skipped() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file).unwrap(); // Empty line
        writeln!(file).unwrap(); // Empty line
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 2);
        assert!(result.errors.is_empty());
        // Position includes empty line bytes
        let expected = (line1.len() + 1 + 1 + 1 + line2.len() + 1) as u64;
        assert_eq!(result.bytes_read, expected);
    }

    #[test]
    fn test_whitespace_only_lines() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "   \t  ").unwrap(); // Whitespace only
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 2); // Whitespace lines skipped
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_errors_contain_byte_positions() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("valid");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "valid line 2").unwrap();
        file.write_all(b"{\"invalid\": json}\n").unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert!(!result.errors.is_empty());
        // Errors now include byte offsets instead of line numbers
        assert_eq!(result.errors.len(), 2); // Two invalid lines
        // Check that errors contain "Parse error at byte"
        let has_byte_offset = result
            .errors
            .iter()
            .all(|e| e.contains("Parse error at byte"));
        assert!(
            has_byte_offset,
            "Expected errors with byte offsets, got: {:?}",
            result.errors
        );
    }

    // ============================================================================
    // 5. Edge Cases
    // ============================================================================

    #[test]
    fn test_very_long_line() {
        let mut file = NamedTempFile::new().unwrap();
        // Create a line with >10KB of content
        let long_text = "a".repeat(15000);
        let line = user_entry(&long_text);
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.bytes_read, (line.len() + 1) as u64);
    }

    #[test]
    fn test_special_characters_in_json() {
        let mut file = NamedTempFile::new().unwrap();
        // JSON with escaped newlines, tabs, quotes
        let line = user_entry(r#"text with \n newline and \t tab and \" quote"#);
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_unicode_boundary_safe() {
        let mut file = NamedTempFile::new().unwrap();
        // Various multi-byte UTF-8 characters
        let line1 = user_entry("日本語");
        let line2 = user_entry("🎉🎊🎈");
        let line3 = user_entry("Ñoño");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        assert_eq!(result.entries.len(), 3);
        assert!(result.errors.is_empty());
        let expected = (line1.len() + 1 + line2.len() + 1 + line3.len() + 1) as u64;
        assert_eq!(result.bytes_read, expected);
    }

    #[test]
    fn test_crlf_line_endings() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        write!(file, "{}\r\n", line).unwrap(); // CRLF instead of LF
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        // StreamDeserializer correctly accounts for all bytes including \r
        // The file has line + \r\n (line.len() + 2 bytes)
        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        // Correct behavior: position includes all bytes including \r
        assert_eq!(result.bytes_read, (line.len() + 2) as u64);
    }

    #[test]
    fn test_crlf_multi_line_incremental_parsing() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");
        let line3 = user_entry("third");

        // Write first line with CRLF
        write!(file, "{}\r\n", line1).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result1.entries.len(), 1);
        assert_eq!(result1.bytes_read, (line1.len() + 2) as u64);
        let pos1 = result1.bytes_read;

        // Append second line with CRLF
        write!(file, "{}\r\n", line2).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_from_position(file.path(), pos1).unwrap();
        assert_eq!(result2.entries.len(), 1);
        assert_eq!(
            result2.bytes_read,
            pos1 + (line2.len() + 2) as u64,
            "Position should correctly account for \\r in second line"
        );
        let pos2 = result2.bytes_read;

        // Append third line with CRLF
        write!(file, "{}\r\n", line3).unwrap();
        file.flush().unwrap();

        let result3 = parse_jsonl_from_position(file.path(), pos2).unwrap();
        assert_eq!(result3.entries.len(), 1);
        assert_eq!(
            result3.bytes_read,
            pos2 + (line3.len() + 2) as u64,
            "Position should correctly account for \\r in third line"
        );

        // Verify complete file parse produces correct total
        let result_full = parse_jsonl_file(file.path()).unwrap();
        assert_eq!(result_full.entries.len(), 3);
        assert_eq!(
            result_full.bytes_read,
            (line1.len() + 2 + line2.len() + 2 + line3.len() + 2) as u64,
            "Full parse should correctly count all CRLF bytes"
        );
    }

    #[test]
    fn test_mixed_valid_unknown_types() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("user message");
        let line2 = serde_json::json!({"type":"unknown_type","data":"something"}).to_string();
        let line3 = assistant_entry("assistant message");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_file(file.path()).unwrap();

        // Unknown types are handled gracefully (no entries, no errors)
        assert_eq!(result.entries.len(), 2); // User and assistant only
        assert!(result.errors.is_empty());
    }
}
