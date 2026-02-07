use anyhow::Result;
use serde_json::error::Category;
use std::fs::File;
use std::io::{Read as _, Seek, SeekFrom};
use std::path::Path;

use super::types::{DisplayEntry, LogEntry};

/// Result of parsing a JSONL file using StreamDeserializer
pub struct StreamParseResult {
    pub entries: Vec<DisplayEntry>,
    /// Parse errors (descriptions, not fatal)
    pub errors: Vec<String>,
    /// Number of bytes read from the file
    pub bytes_read: u64,
}

/// Parse a JSONL file from the beginning using StreamDeserializer
pub fn parse_jsonl_stream(path: &Path) -> Result<StreamParseResult> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    parse_stream_content(&content, 0)
}

/// Parse a JSONL file from a specific position using StreamDeserializer
///
/// Note: This requires re-parsing from the start to reach the position,
/// which is O(n) compared to the seek-based approach's O(1).
pub fn parse_jsonl_stream_from_position(path: &Path, position: u64) -> Result<StreamParseResult> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(position))?;

    let mut content = String::new();
    file.read_to_string(&mut content)?;

    parse_stream_content(&content, position)
}

fn parse_stream_content(content: &str, base_position: u64) -> Result<StreamParseResult> {
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
                last_valid_position = current_pos + stream.byte_offset();
                current_pos = last_valid_position;
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

    Ok(StreamParseResult {
        entries,
        errors,
        bytes_read: base_position + last_valid_position as u64,
    })
}

fn convert_log_entry(entry: &LogEntry) -> Vec<DisplayEntry> {
    // Reuse the conversion logic from the main parser
    super::parser::convert_log_entry(entry)
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
    // 1. Byte Position Accuracy Tests
    // ============================================================================

    #[test]
    fn test_position_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let result = parse_jsonl_stream(file.path()).unwrap();

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

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        // StreamDeserializer should track position after the JSON value
        // For JSONL, this means after the closing brace, but before the newline
        assert_eq!(result.bytes_read, line.len() as u64);
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

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert_eq!(result.entries.len(), 3);
        assert!(result.errors.is_empty());
        // Position should be after the last JSON value
        let expected_bytes = (line1.len() + 1 + line2.len() + 1 + line3.len()) as u64;
        assert_eq!(result.bytes_read, expected_bytes);
    }

    #[test]
    fn test_position_with_utf8() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("hello 😀");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        // Verify byte_offset() handles UTF-8 correctly
        assert_eq!(result.bytes_read, line.len() as u64);
    }

    #[test]
    fn test_position_accumulates_correctly() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_stream(file.path()).unwrap();
        let pos1 = result1.bytes_read;

        // Append a second line
        let line2 = user_entry("second");
        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_stream_from_position(file.path(), pos1).unwrap();

        // Position should be absolute
        assert!(result2.bytes_read > pos1);
    }

    // ============================================================================
    // 2. Incomplete JSON Detection Tests
    // ============================================================================

    #[test]
    fn test_incomplete_json_at_eof() {
        let mut file = NamedTempFile::new().unwrap();
        // Incomplete JSON without newline
        file.write_all(br#"{"type":"user","message":{"#).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert!(result.entries.is_empty());
        // Should detect EOF error and not advance position
        assert_eq!(result.bytes_read, 0);
    }

    #[test]
    fn test_incomplete_then_complete() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_stream(file.path()).unwrap();
        assert_eq!(result1.entries.len(), 1);
        let pos1 = result1.bytes_read;

        // Write incomplete JSON
        file.write_all(br#"{"type":"user","message":{"#).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_stream(file.path()).unwrap();
        // Should still only have 1 entry, position unchanged
        assert_eq!(result2.entries.len(), 1);
        assert_eq!(result2.bytes_read, pos1);

        // Complete the line
        file.write_all(b"\"role\":\"user\",\"content\":\"hello\"}}\n")
            .unwrap();
        file.flush().unwrap();

        let result3 = parse_jsonl_stream(file.path()).unwrap();
        assert_eq!(result3.entries.len(), 2);
    }

    // ============================================================================
    // 3. Syntax Error vs EOF Error Tests
    // ============================================================================

    #[test]
    fn test_malformed_json_with_newline() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"{\"invalid\": json}\n").unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert!(result.entries.is_empty());
        assert_eq!(result.errors.len(), 1);
        // Position should advance past the malformed line
        assert!(result.bytes_read > 0);
    }

    #[test]
    fn test_eof_vs_syntax_error() {
        // Test 1: Incomplete JSON (EOF error)
        let mut file1 = NamedTempFile::new().unwrap();
        file1.write_all(br#"{"type":"user"#).unwrap();
        file1.flush().unwrap();

        let result1 = parse_jsonl_stream(file1.path()).unwrap();
        assert_eq!(result1.bytes_read, 0); // Position not advanced for EOF

        // Test 2: Complete malformed JSON (syntax error)
        let mut file2 = NamedTempFile::new().unwrap();
        file2.write_all(b"{\"type\":invalid}\n").unwrap();
        file2.flush().unwrap();

        let result2 = parse_jsonl_stream(file2.path()).unwrap();
        assert!(result2.bytes_read > 0); // Position advanced past syntax error
    }

    // ============================================================================
    // 4. Error Recovery Tests
    // ============================================================================

    #[test]
    fn test_errors_dont_stop_parsing() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("valid");
        writeln!(file, "{}", line1).unwrap();
        file.write_all(b"{\"invalid\": json}\n").unwrap();
        let line3 = assistant_entry("also valid");
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        // Should parse both valid entries and record the error
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_multiple_errors_recovery() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        writeln!(file, "{}", line1).unwrap();
        file.write_all(b"{\"bad1\": json}\n").unwrap();
        let line3 = user_entry("second");
        writeln!(file, "{}", line3).unwrap();
        file.write_all(b"{\"bad2\": also bad}\n").unwrap();
        let line5 = user_entry("third");
        writeln!(file, "{}", line5).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.errors.len(), 2);
    }

    // ============================================================================
    // 5. Resumption Simulation Tests
    // ============================================================================

    #[test]
    fn test_resume_from_middle() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("first");
        let line2 = user_entry("second");

        writeln!(file, "{}", line1).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_stream(file.path()).unwrap();
        let pos = result1.bytes_read;

        writeln!(file, "{}", line2).unwrap();
        file.flush().unwrap();

        let result2 = parse_jsonl_stream_from_position(file.path(), pos).unwrap();

        assert_eq!(result2.entries.len(), 1); // Only new entry
    }

    #[test]
    fn test_resume_preserves_position_on_empty_read() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result1 = parse_jsonl_stream(file.path()).unwrap();
        let pos = result1.bytes_read;

        // Parse again from EOF without new data
        let result2 = parse_jsonl_stream_from_position(file.path(), pos).unwrap();

        assert!(result2.entries.is_empty());
        assert_eq!(result2.bytes_read, pos);
    }

    // ============================================================================
    // 6. Edge Cases
    // ============================================================================

    #[test]
    fn test_very_long_line() {
        let mut file = NamedTempFile::new().unwrap();
        let long_text = "a".repeat(15000);
        let line = user_entry(&long_text);
        writeln!(file, "{}", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_unicode_boundary_safe() {
        let mut file = NamedTempFile::new().unwrap();
        let line1 = user_entry("日本語");
        let line2 = user_entry("🎉🎊🎈");
        let line3 = user_entry("Ñoño");
        writeln!(file, "{}", line1).unwrap();
        writeln!(file, "{}", line2).unwrap();
        writeln!(file, "{}", line3).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert_eq!(result.entries.len(), 3);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_crlf_line_endings() {
        let mut file = NamedTempFile::new().unwrap();
        let line = user_entry("test");
        write!(file, "{}\r\n", line).unwrap();
        file.flush().unwrap();

        let result = parse_jsonl_stream(file.path()).unwrap();

        assert_eq!(result.entries.len(), 1);
        // Document actual behavior with CRLF
    }
}
