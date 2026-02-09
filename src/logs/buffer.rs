use anyhow::Result;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::index::LineIndex;
use super::parser::{ParseResult, merge_tool_results, parse_jsonl_range};
use super::types::{DisplayEntry, ToolCallResult};

/// Direction of load operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadDirection {
    Older,   // prepending older entries
    Newer,   // appending newer entries
    Replace, // replacing buffer (jump to start/end)
}

/// Pending async load request
#[derive(Debug, Clone)]
struct PendingLoad {
    /// Target JSONL line range being loaded
    target_start: usize,
    target_end: usize,
    /// Whether this load prepends (older) or replaces/appends
    direction: LoadDirection,
}

/// Windowed entry buffer with demand-driven loading
pub struct EntryBuffer {
    index: LineIndex,
    entries: VecDeque<DisplayEntry>,
    /// JSONL line index of the first entry in the buffer
    window_start_line: usize,
    /// JSONL line index of the last entry in the buffer (inclusive)
    window_end_line: usize,
    /// Max JSONL lines to keep parsed in the buffer
    capacity: usize,
    path: PathBuf,
    parse_errors: Vec<String>,
    /// In-flight async load request
    pending_load: Option<PendingLoad>,
    /// Last time a load was requested (for rate limiting)
    last_load_time: Option<std::time::Instant>,
}

impl EntryBuffer {
    /// Create a new EntryBuffer with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            index: LineIndex::build(&PathBuf::new()).unwrap_or_else(|_| {
                // Empty index for uninitialized state
                LineIndex::build(&PathBuf::from("/dev/null")).unwrap()
            }),
            entries: VecDeque::new(),
            window_start_line: 0,
            window_end_line: 0,
            capacity,
            path: PathBuf::new(),
            parse_errors: Vec::new(),
            pending_load: None,
            last_load_time: None,
        }
    }

    /// Load a new file. Builds index, loads tail entries (for follow mode start).
    /// Synchronous — index scan is microseconds, parsing ≤capacity lines is <5ms.
    pub fn load_file(&mut self, path: &Path) -> Result<()> {
        // Build index
        self.index = LineIndex::build(path)?;
        self.path = path.to_path_buf();
        self.entries.clear();
        self.parse_errors.clear();
        self.pending_load = None;

        let total_lines = self.index.line_count();
        if total_lines == 0 {
            self.window_start_line = 0;
            self.window_end_line = 0;
            return Ok(());
        }

        // Load tail entries (last `capacity` lines)
        let start_line = total_lines.saturating_sub(self.capacity);
        let end_line = total_lines;

        let (start_byte, end_byte) = self
            .index
            .range_byte_range(start_line, end_line)
            .ok_or_else(|| anyhow::anyhow!("Invalid line range"))?;

        let parse_result = parse_jsonl_range(path, start_byte, end_byte)?;
        self.parse_errors = parse_result.errors;

        let merged = merge_tool_results(parse_result.entries);
        self.entries = VecDeque::from(merged);
        self.window_start_line = start_line;
        self.window_end_line = end_line.saturating_sub(1);

        Ok(())
    }

    /// File changed (watcher event). Updates index.
    /// If follow_mode, parses and appends new entries, evicts old from front.
    /// Returns count of new entries added.
    /// Synchronous — tail updates parse only the few new lines.
    pub fn file_changed(&mut self, follow_mode: bool) -> Result<usize> {
        let new_line_count = self.index.update(&self.path)?;

        if new_line_count == 0 {
            return Ok(0);
        }

        if !follow_mode {
            // Not following - don't auto-load new content
            return Ok(0);
        }

        // Parse new lines
        let total_lines = self.index.line_count();
        let old_end = self.window_end_line + 1;
        let new_end = total_lines;

        if old_end >= new_end {
            return Ok(0);
        }

        let (start_byte, end_byte) = self
            .index
            .range_byte_range(old_end, new_end)
            .ok_or_else(|| anyhow::anyhow!("Invalid line range"))?;

        let parse_result = parse_jsonl_range(&self.path, start_byte, end_byte)?;
        self.parse_errors.extend(parse_result.errors);

        let merged_new = merge_tool_results(parse_result.entries);
        let new_count = merged_new.len();

        // Check for tool result merging at boundary
        if let Some(DisplayEntry::ToolCall { id, result, .. }) = self.entries.back_mut()
            && result.is_none()
            && let Some(DisplayEntry::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            }) = merged_new.first()
            && tool_use_id == id
        {
            // Merge the result into the existing ToolCall
            *result = Some(ToolCallResult {
                content: content.clone(),
                is_error: *is_error,
            });
            // Skip the first entry since we merged it
            self.entries.extend(merged_new.into_iter().skip(1));
        } else {
            self.entries.extend(merged_new);
        }

        self.window_end_line = new_end.saturating_sub(1);

        // Evict old entries from front if over capacity
        let total_buffered = (self.window_end_line + 1).saturating_sub(self.window_start_line);
        if total_buffered > self.capacity {
            let to_evict = total_buffered - self.capacity;
            for _ in 0..to_evict {
                self.entries.pop_front();
                self.window_start_line += 1;
            }
        }

        Ok(new_count)
    }

    /// Access the current entries for rendering
    pub fn entries(&self) -> &VecDeque<DisplayEntry> {
        &self.entries
    }

    /// Whether there are older entries available beyond the buffer
    pub fn has_older(&self) -> bool {
        self.window_start_line > 0
    }

    /// Whether there are newer entries available beyond the buffer
    pub fn has_newer(&self) -> bool {
        let total_lines = self.index.line_count();
        if total_lines == 0 {
            return false;
        }
        self.window_end_line + 1 < total_lines
    }

    /// Total JSONL lines in the file (for approximate scrollbar)
    pub fn total_file_lines(&self) -> usize {
        self.index.line_count()
    }

    /// Current window position as (start_line, end_line) for scrollbar
    pub fn window_position(&self) -> (usize, usize) {
        (self.window_start_line, self.window_end_line)
    }

    /// Whether an async load is in flight
    pub fn is_loading(&self) -> bool {
        self.pending_load.is_some()
    }

    /// Parse errors encountered
    pub fn parse_errors(&self) -> &[String] {
        &self.parse_errors
    }

    /// Request loading older entries. Returns None if already loading or nothing to load.
    /// Returns Some((path, byte_start, byte_end)) for the caller to spawn a parse task.
    pub fn request_load_older(&mut self, count: usize) -> Option<(PathBuf, u64, u64)> {
        // Rate limit: don't trigger loads more than once per 50ms
        if let Some(last_time) = self.last_load_time
            && last_time.elapsed() < std::time::Duration::from_millis(50) {
                return None;
            }

        if self.pending_load.is_some() || !self.has_older() {
            return None;
        }

        let target_end = self.window_start_line;
        let target_start = target_end.saturating_sub(count);

        let (start_byte, end_byte) = self.index.range_byte_range(target_start, target_end)?;

        self.pending_load = Some(PendingLoad {
            target_start,
            target_end,
            direction: LoadDirection::Older,
        });
        self.last_load_time = Some(std::time::Instant::now());

        Some((self.path.clone(), start_byte, end_byte))
    }

    /// Request loading newer entries
    pub fn request_load_newer(&mut self, count: usize) -> Option<(PathBuf, u64, u64)> {
        // Rate limit: don't trigger loads more than once per 50ms
        if let Some(last_time) = self.last_load_time
            && last_time.elapsed() < std::time::Duration::from_millis(50) {
                return None;
            }

        if self.pending_load.is_some() || !self.has_newer() {
            return None;
        }

        let target_start = self.window_end_line + 1;
        let target_end = (target_start + count).min(self.index.line_count());

        let (start_byte, end_byte) = self.index.range_byte_range(target_start, target_end)?;

        self.pending_load = Some(PendingLoad {
            target_start,
            target_end,
            direction: LoadDirection::Newer,
        });
        self.last_load_time = Some(std::time::Instant::now());

        Some((self.path.clone(), start_byte, end_byte))
    }

    /// Request jump to file start. Returns parse parameters.
    pub fn request_jump_to_start(&mut self) -> Option<(PathBuf, u64, u64)> {
        // No rate limiting for explicit jumps
        if self.pending_load.is_some() {
            return None;
        }

        let target_start = 0;
        let target_end = self.capacity.min(self.index.line_count());

        if target_end == 0 {
            return None;
        }

        let (start_byte, end_byte) = self.index.range_byte_range(target_start, target_end)?;

        self.pending_load = Some(PendingLoad {
            target_start,
            target_end,
            direction: LoadDirection::Replace,
        });
        self.last_load_time = Some(std::time::Instant::now());

        Some((self.path.clone(), start_byte, end_byte))
    }

    /// Request jump to file end. Returns parse parameters.
    pub fn request_jump_to_end(&mut self) -> Option<(PathBuf, u64, u64)> {
        // No rate limiting for explicit jumps
        if self.pending_load.is_some() {
            return None;
        }

        let total_lines = self.index.line_count();
        if total_lines == 0 {
            return None;
        }

        let target_start = total_lines.saturating_sub(self.capacity);
        let target_end = total_lines;

        let (start_byte, end_byte) = self.index.range_byte_range(target_start, target_end)?;

        self.pending_load = Some(PendingLoad {
            target_start,
            target_end,
            direction: LoadDirection::Replace,
        });
        self.last_load_time = Some(std::time::Instant::now());

        Some((self.path.clone(), start_byte, end_byte))
    }

    /// Receive results from an async parse. Updates buffer, returns
    /// scroll_offset adjustment (positive = shift down, negative = shift up).
    /// content_width needed to calculate rendered line counts.
    pub fn receive_loaded(
        &mut self,
        result: Result<ParseResult>,
        content_width: usize,
        show_thinking: bool,
        expand_tools: bool,
    ) -> isize {
        let Some(pending) = self.pending_load.take() else {
            return 0;
        };

        let Ok(parse_result) = result else {
            return 0;
        };

        self.parse_errors.extend(parse_result.errors);
        let merged = merge_tool_results(parse_result.entries);

        match pending.direction {
            LoadDirection::Older => {
                // Prepending older entries - only prepend count matters for scroll adjustment
                let added_count =
                    calculate_entries_lines(&merged, content_width, show_thinking, expand_tools);

                // Check for tool result merging at boundary
                if let Some(DisplayEntry::ToolCall { id, result, .. }) = merged.last()
                    && result.is_none()
                    && let Some(DisplayEntry::ToolResult {
                        tool_use_id,
                        content: _,
                        is_error: _,
                        ..
                    }) = self.entries.front()
                    && tool_use_id == id
                {
                    // Can't easily merge backward - would need mutable access to merged vec
                    // For now, skip this edge case (rare - requires load boundary exactly between tool call and result)
                }

                // Prepend to buffer
                for entry in merged.into_iter().rev() {
                    self.entries.push_front(entry);
                }
                self.window_start_line = pending.target_start;

                // Evict from back if over capacity (doesn't affect scroll position)
                let total_buffered =
                    (self.window_end_line + 1).saturating_sub(self.window_start_line);
                if total_buffered > self.capacity {
                    let to_evict = total_buffered - self.capacity;
                    for _ in 0..to_evict {
                        self.entries.pop_back();
                        self.window_end_line = self.window_end_line.saturating_sub(1);
                    }
                }

                added_count as isize // Positive = shift scroll down
            }
            LoadDirection::Newer => {
                // Appending newer entries - only front eviction matters for scroll adjustment

                // Check for tool result merging at boundary
                if let Some(DisplayEntry::ToolCall { id, result, .. }) = self.entries.back_mut()
                    && result.is_none()
                    && let Some(DisplayEntry::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    }) = merged.first()
                    && tool_use_id == id
                {
                    *result = Some(ToolCallResult {
                        content: content.clone(),
                        is_error: *is_error,
                    });
                    self.entries.extend(merged.into_iter().skip(1));
                } else {
                    self.entries.extend(merged);
                }

                self.window_end_line = pending.target_end.saturating_sub(1);

                // Evict from front if over capacity - this shifts content up
                let total_buffered =
                    (self.window_end_line + 1).saturating_sub(self.window_start_line);
                let mut evicted_count = 0;
                if total_buffered > self.capacity {
                    let to_evict = total_buffered - self.capacity;
                    for _ in 0..to_evict {
                        if let Some(entry) = self.entries.pop_front() {
                            evicted_count += calculate_entry_lines(
                                &entry,
                                content_width,
                                show_thinking,
                                expand_tools,
                            );
                            self.window_start_line += 1;
                        }
                    }
                }

                -(evicted_count as isize) // Negative = shift scroll up
            }
            LoadDirection::Replace => {
                // Replace entire buffer - caller should reset scroll_offset
                self.entries.clear();
                self.entries = VecDeque::from(merged);
                self.window_start_line = pending.target_start;
                self.window_end_line = pending.target_end.saturating_sub(1);

                0 // No adjustment - caller resets scroll
            }
        }
    }
}

/// Calculate rendered lines for a single entry
fn calculate_entry_lines(
    entry: &DisplayEntry,
    content_width: usize,
    show_thinking: bool,
    expand_tools: bool,
) -> usize {
    match entry {
        DisplayEntry::UserMessage { text, .. } => 1 + wrap_text_line_count(text, content_width) + 1,
        DisplayEntry::AssistantText { text, .. } => {
            1 + wrap_text_line_count(text, content_width) + 1
        }
        DisplayEntry::ToolCall {
            name,
            input,
            result,
            ..
        } => {
            let mut count = 1; // Tool name line
            if expand_tools {
                // Estimate based on typical tool rendering
                count += estimate_tool_lines(name, input, content_width);
            }
            if let Some(res) = result {
                if expand_tools {
                    count += 1; // separator
                    count += wrap_text_line_count(&res.content, content_width).min(10);
                } else {
                    count += 1; // collapsed result indicator
                }
            }
            count + 1 // blank line
        }
        DisplayEntry::ToolResult { content, .. } => {
            let mut count = 1;
            if expand_tools && !content.is_empty() {
                count += wrap_text_line_count(content, content_width).min(10);
            }
            count + 1
        }
        DisplayEntry::Thinking { text, .. } => {
            if show_thinking {
                1 + wrap_text_line_count(text, content_width) + 1
            } else {
                1 // collapsed indicator
            }
        }
        DisplayEntry::HookEvent { .. } => 2,
        DisplayEntry::AgentSpawn { .. } => 2,
    }
}

/// Calculate total rendered lines for multiple entries
fn calculate_entries_lines(
    entries: &[DisplayEntry],
    content_width: usize,
    show_thinking: bool,
    expand_tools: bool,
) -> usize {
    entries
        .iter()
        .map(|entry| calculate_entry_lines(entry, content_width, show_thinking, expand_tools))
        .sum()
}

/// Count lines needed to wrap text
fn wrap_text_line_count(text: &str, width: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut count = 0;
    for line in text.lines() {
        if line.is_empty() {
            count += 1;
        } else {
            let line_len = unicode_width::UnicodeWidthStr::width(line);
            count += (line_len + width - 1) / width.max(1);
        }
    }
    count
}

/// Estimate lines for tool rendering
fn estimate_tool_lines(name: &str, input: &str, content_width: usize) -> usize {
    match name {
        "Bash" | "Read" | "Write" | "Edit" | "Grep" | "Glob" => {
            // Typical tools show 1-3 lines of info
            2
        }
        "Task" | "TodoWrite" => {
            // These can be longer
            3 + wrap_text_line_count(input, content_width).min(5)
        }
        _ => {
            // Generic tool - show full input if short
            2 + wrap_text_line_count(input, content_width).min(10)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

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

    #[test]
    fn test_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let mut buffer = EntryBuffer::new(100);

        buffer.load_file(file.path()).unwrap();

        assert_eq!(buffer.entries().len(), 0);
        assert_eq!(buffer.total_file_lines(), 0);
        assert!(!buffer.has_older());
        assert!(!buffer.has_newer());
    }

    #[test]
    fn test_load_small_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", user_entry("line 1")).unwrap();
        writeln!(file, "{}", user_entry("line 2")).unwrap();
        writeln!(file, "{}", user_entry("line 3")).unwrap();
        file.flush().unwrap();

        let mut buffer = EntryBuffer::new(100);
        buffer.load_file(file.path()).unwrap();

        assert_eq!(buffer.entries().len(), 3);
        assert_eq!(buffer.total_file_lines(), 3);
        assert!(!buffer.has_older());
        assert!(!buffer.has_newer());
    }

    #[test]
    fn test_load_tail_only() {
        let mut file = NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(file, "{}", user_entry(&format!("line {}", i))).unwrap();
        }
        file.flush().unwrap();

        let mut buffer = EntryBuffer::new(5);
        buffer.load_file(file.path()).unwrap();

        assert_eq!(buffer.entries().len(), 5);
        assert_eq!(buffer.total_file_lines(), 10);
        assert!(buffer.has_older());
        assert!(!buffer.has_newer());
    }

    #[test]
    fn test_file_changed_follow_mode() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", user_entry("line 1")).unwrap();
        file.flush().unwrap();

        let mut buffer = EntryBuffer::new(100);
        buffer.load_file(file.path()).unwrap();
        assert_eq!(buffer.entries().len(), 1);

        // Append new line
        writeln!(file, "{}", user_entry("line 2")).unwrap();
        file.flush().unwrap();

        let added = buffer.file_changed(true).unwrap();
        assert_eq!(added, 1);
        assert_eq!(buffer.entries().len(), 2);
    }

    #[test]
    fn test_file_changed_not_following() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", user_entry("line 1")).unwrap();
        file.flush().unwrap();

        let mut buffer = EntryBuffer::new(100);
        buffer.load_file(file.path()).unwrap();

        writeln!(file, "{}", user_entry("line 2")).unwrap();
        file.flush().unwrap();

        let added = buffer.file_changed(false).unwrap();
        assert_eq!(added, 0);
        assert_eq!(buffer.entries().len(), 1);
    }

    #[test]
    fn test_request_load_older() {
        let mut file = NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(file, "{}", user_entry(&format!("line {}", i))).unwrap();
        }
        file.flush().unwrap();

        let mut buffer = EntryBuffer::new(5);
        buffer.load_file(file.path()).unwrap();
        assert!(buffer.has_older());

        let request = buffer.request_load_older(3);
        assert!(request.is_some());
        assert!(buffer.is_loading());

        // Can't request another load while one is pending
        let request2 = buffer.request_load_older(3);
        assert!(request2.is_none());
    }

    #[test]
    fn test_request_load_newer() {
        let mut file = NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(file, "{}", user_entry(&format!("line {}", i))).unwrap();
        }
        file.flush().unwrap();

        let mut buffer = EntryBuffer::new(5);
        buffer.load_file(file.path()).unwrap();

        // Simulate scrolling to beginning
        buffer.window_start_line = 0;
        buffer.window_end_line = 4;

        assert!(buffer.has_newer());
        let request = buffer.request_load_newer(3);
        assert!(request.is_some());
        assert!(buffer.is_loading());
    }
}
