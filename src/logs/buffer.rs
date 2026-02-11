use anyhow::Result;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::index::LineIndex;
use super::parser::{ParseResult, merge_tool_results, parse_jsonl_range};
use super::types::{DisplayEntry, ToolCallResult};
use crate::ui::conversation::calculate_entry_lines;

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
    /// How many JSONL lines each display entry consumed (1 normally, 2 for merged ToolCall+ToolResult)
    source_lines: VecDeque<usize>,
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

/// Compute source_lines from merged entries, given the JSONL line count that produced them.
/// Each ToolCall with result.is_some() consumed 2 JSONL lines; everything else consumed 1.
fn compute_source_lines(entries: &[DisplayEntry], jsonl_count: usize) -> VecDeque<usize> {
    let mut lines: VecDeque<usize> = entries
        .iter()
        .map(|e| match e {
            DisplayEntry::ToolCall {
                result: Some(_), ..
            } => 2,
            _ => 1,
        })
        .collect();
    // Adjust if sum doesn't match jsonl_count (edge cases)
    let sum: usize = lines.iter().sum();
    if sum != jsonl_count
        && !lines.is_empty()
        && let Some(last) = lines.back_mut()
    {
        *last = last.saturating_add(jsonl_count.saturating_sub(sum));
    }
    lines
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
            source_lines: VecDeque::new(),
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
        self.source_lines.clear();
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
        let jsonl_count = end_line - start_line;
        self.source_lines = compute_source_lines(&merged, jsonl_count);
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
        let new_jsonl_count = new_end - old_end;
        let mut new_source_lines = compute_source_lines(&merged_new, new_jsonl_count);

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
            // Update source_lines: last existing entry now covers its original line + the merged result line
            if let Some(last_sl) = self.source_lines.back_mut()
                && let Some(first_new_sl) = new_source_lines.pop_front()
            {
                *last_sl += first_new_sl;
            }
            // Skip the first entry since we merged it
            self.entries.extend(merged_new.into_iter().skip(1));
            self.source_lines.extend(new_source_lines);
        } else {
            self.entries.extend(merged_new);
            self.source_lines.extend(new_source_lines);
        }

        self.window_end_line = new_end.saturating_sub(1);

        // Evict old entries from front if over capacity
        let total_buffered = (self.window_end_line + 1).saturating_sub(self.window_start_line);
        if total_buffered > self.capacity {
            let jsonl_to_evict = total_buffered - self.capacity;
            let mut evicted_jsonl = 0;
            while evicted_jsonl < jsonl_to_evict && !self.entries.is_empty() {
                self.entries.pop_front();
                let sl = self.source_lines.pop_front().unwrap_or(1);
                evicted_jsonl += sl;
            }
            self.window_start_line += evicted_jsonl;
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

    /// Total rendered lines for all buffered entries
    pub fn total_rendered_lines(
        &self,
        content_width: usize,
        show_thinking: bool,
        expand_tools: bool,
    ) -> usize {
        self.entries
            .iter()
            .map(|entry| calculate_entry_lines(entry, content_width, show_thinking, expand_tools))
            .sum()
    }

    /// Whether an async load is in flight
    pub fn is_loading(&self) -> bool {
        self.pending_load.is_some()
    }

    /// Parse errors encountered
    pub fn parse_errors(&self) -> &[String] {
        &self.parse_errors
    }

    /// Clear rate limit for testing (allows rapid successive loads)
    #[doc(hidden)]
    pub fn clear_rate_limit(&mut self) {
        self.last_load_time = None;
    }

    /// Request loading older entries. Returns None if already loading or nothing to load.
    /// Returns Some((path, byte_start, byte_end)) for the caller to spawn a parse task.
    pub fn request_load_older(&mut self, count: usize) -> Option<(PathBuf, u64, u64)> {
        // Rate limit: don't trigger loads more than once per 50ms
        if let Some(last_time) = self.last_load_time
            && last_time.elapsed() < std::time::Duration::from_millis(50)
        {
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
            && last_time.elapsed() < std::time::Duration::from_millis(50)
        {
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
        let jsonl_count = pending.target_end.saturating_sub(pending.target_start);

        match pending.direction {
            LoadDirection::Older => {
                // Prepending older entries - only prepend count matters for scroll adjustment

                if merged.is_empty() {
                    // Advance window even when parse produced no entries
                    self.window_start_line = pending.target_start;
                    return 0;
                }

                let added_count =
                    calculate_entries_lines(&merged, content_width, show_thinking, expand_tools);

                let new_source_lines = compute_source_lines(&merged, jsonl_count);

                // Prepend source_lines and entries
                for sl in new_source_lines.into_iter().rev() {
                    self.source_lines.push_front(sl);
                }
                for entry in merged.into_iter().rev() {
                    self.entries.push_front(entry);
                }
                self.window_start_line = pending.target_start;

                // Evict from back if over capacity (doesn't affect scroll position)
                let total_buffered =
                    (self.window_end_line + 1).saturating_sub(self.window_start_line);
                if total_buffered > self.capacity {
                    let jsonl_to_evict = total_buffered - self.capacity;
                    let mut evicted_jsonl = 0;
                    while evicted_jsonl < jsonl_to_evict && !self.entries.is_empty() {
                        self.entries.pop_back();
                        let sl = self.source_lines.pop_back().unwrap_or(1);
                        evicted_jsonl += sl;
                    }
                    self.window_end_line = self.window_end_line.saturating_sub(evicted_jsonl);
                }

                added_count as isize // Positive = shift scroll down
            }
            LoadDirection::Newer => {
                // Appending newer entries - only front eviction matters for scroll adjustment

                if merged.is_empty() {
                    // Advance window even when parse produced no entries
                    self.window_end_line = pending.target_end.saturating_sub(1);
                    return 0;
                }

                let mut new_source_lines = compute_source_lines(&merged, jsonl_count);

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
                    // Update source_lines: absorb the first new entry's lines into the last existing
                    if let Some(last_sl) = self.source_lines.back_mut()
                        && let Some(first_new_sl) = new_source_lines.pop_front()
                    {
                        *last_sl += first_new_sl;
                    }
                    self.entries.extend(merged.into_iter().skip(1));
                    self.source_lines.extend(new_source_lines);
                } else {
                    self.entries.extend(merged);
                    self.source_lines.extend(new_source_lines);
                }

                self.window_end_line = pending.target_end.saturating_sub(1);

                // Evict from front if over capacity - this shifts content up
                let mut evicted_count = 0;
                let total_buffered =
                    (self.window_end_line + 1).saturating_sub(self.window_start_line);
                if total_buffered > self.capacity {
                    let jsonl_to_evict = total_buffered - self.capacity;
                    let mut evicted_jsonl = 0;
                    while evicted_jsonl < jsonl_to_evict && !self.entries.is_empty() {
                        if let Some(entry) = self.entries.pop_front() {
                            evicted_count += calculate_entry_lines(
                                &entry,
                                content_width,
                                show_thinking,
                                expand_tools,
                            );
                        }
                        let sl = self.source_lines.pop_front().unwrap_or(1);
                        evicted_jsonl += sl;
                    }
                    self.window_start_line += evicted_jsonl;
                }

                -(evicted_count as isize) // Negative = shift scroll up
            }
            LoadDirection::Replace => {
                // Replace entire buffer - caller should reset scroll_offset
                self.entries.clear();
                self.source_lines.clear();
                let new_source_lines = compute_source_lines(&merged, jsonl_count);
                self.source_lines = new_source_lines;
                self.entries = VecDeque::from(merged);
                self.window_start_line = pending.target_start;
                self.window_end_line = pending.target_end.saturating_sub(1);

                0 // No adjustment - caller resets scroll
            }
        }
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

    fn tool_use_entry(id: &str, name: &str, input: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": { "command": input }
                }]
            }
        })
        .to_string()
    }

    fn tool_result_entry(tool_use_id: &str, content: &str) -> String {
        serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": false
                }]
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

    /// Build a file with tool call pairs interleaved with messages.
    /// If long_text is true, generates realistic long content that stresses text wrapping.
    /// Returns (file, total_jsonl_lines).
    fn build_tool_file_ex(line_count: usize, long_text: bool) -> (NamedTempFile, usize) {
        let mut file = NamedTempFile::new().unwrap();
        let mut total = 0;
        for i in 0..line_count {
            if i % 3 == 0 {
                // Tool call pair = 2 JSONL lines
                let id = format!("toolu_{:010}", i);
                let cmd = if long_text {
                    format!(
                        "grep -r 'function.*{}' src/ --include='*.ts' --include='*.tsx' | head -50 && echo 'searching for pattern {} in the codebase to find all matching functions and their locations'",
                        i, i
                    )
                } else {
                    format!("cmd {}", i)
                };
                let output = if long_text {
                    format!(
                        "src/components/Widget{}.tsx:42:  export function handleUpdate{}(state: AppState, action: Action): Result<AppState, Error> {{\nsrc/components/Widget{}.tsx:85:  function processEvent{}(event: Event, context: Context): void {{\nsrc/utils/helpers{}.ts:12:  export function formatData{}(input: RawData[], options?: FormatOptions): FormattedOutput[] {{",
                        i, i, i, i, i, i
                    )
                } else {
                    format!("output {}", i)
                };
                writeln!(file, "{}", tool_use_entry(&id, "Bash", &cmd)).unwrap();
                writeln!(file, "{}", tool_result_entry(&id, &output)).unwrap();
                total += 2;
            } else if i % 3 == 1 {
                let msg = if long_text {
                    format!(
                        "Can you help me debug this issue? When I run the test suite for module {} it fails with an assertion error on line {}. The expected output was supposed to match the snapshot but the formatting seems different. I've tried running it with different flags but nothing works.",
                        i,
                        i * 10
                    )
                } else {
                    format!("msg {}", i)
                };
                writeln!(file, "{}", user_entry(&msg)).unwrap();
                total += 1;
            } else {
                let reply = if long_text {
                    format!(
                        "I'll help you debug that test failure in module {}. The assertion error on line {} suggests a formatting mismatch. This commonly happens when the snapshot was generated with a different version of the formatter. Let me check the test configuration and the snapshot file to identify the exact discrepancy. First, let me look at the test file to understand what's being tested.",
                        i,
                        i * 10
                    )
                } else {
                    format!("reply {}", i)
                };
                writeln!(file, "{}", assistant_entry(&reply)).unwrap();
                total += 1;
            }
        }
        file.flush().unwrap();
        (file, total)
    }

    fn build_tool_file(line_count: usize) -> (NamedTempFile, usize) {
        build_tool_file_ex(line_count, false)
    }

    #[test]
    fn test_load_older_with_tool_merging_tracks_window() {
        // 20 groups = mix of tool pairs (2 lines) and messages (1 line)
        let (file, total_jsonl) = build_tool_file(20);

        let mut buffer = EntryBuffer::new(8);
        buffer.load_file(file.path()).unwrap();

        assert_eq!(buffer.total_file_lines(), total_jsonl);
        assert!(buffer.has_older());

        let (_, initial_end) = buffer.window_position();

        // Load older
        buffer.clear_rate_limit();
        if let Some((path, start, end)) = buffer.request_load_older(5) {
            let result = parse_jsonl_range(&path, start, end);
            buffer.receive_loaded(result, 80, false, false);
        }

        let (new_start, new_end) = buffer.window_position();

        // Window start should have moved backward
        assert!(new_start < initial_end, "window_start should move backward");
        // Window end may have shrunk due to eviction, but should still be valid
        assert!(new_end >= new_start, "window_end >= window_start");
        // source_lines should be in sync with entries
        assert_eq!(
            buffer.source_lines.len(),
            buffer.entries().len(),
            "source_lines and entries must stay in sync"
        );
        // Sum of source_lines should equal the JSONL window range
        let source_sum: usize = buffer.source_lines.iter().sum();
        let expected_jsonl = (new_end + 1).saturating_sub(new_start);
        assert_eq!(
            source_sum, expected_jsonl,
            "source_lines sum ({}) should equal JSONL window size ({})",
            source_sum, expected_jsonl
        );
    }

    #[test]
    fn test_load_newer_with_tool_merging_tracks_window() {
        let (file, _total_jsonl) = build_tool_file(20);

        // Load from start (via jump to start)
        let mut buffer = EntryBuffer::new(8);
        buffer.load_file(file.path()).unwrap();

        // Jump to start
        if let Some((path, start, end)) = buffer.request_jump_to_start() {
            let result = parse_jsonl_range(&path, start, end);
            buffer.receive_loaded(result, 80, false, false);
        }

        assert!(buffer.has_newer(), "should have newer content");
        let (initial_start, _) = buffer.window_position();

        // Load newer
        buffer.clear_rate_limit();
        if let Some((path, start, end)) = buffer.request_load_newer(5) {
            let result = parse_jsonl_range(&path, start, end);
            buffer.receive_loaded(result, 80, false, false);
        }

        let (new_start, new_end) = buffer.window_position();

        // Window end should have advanced
        assert!(new_end > initial_start, "window_end should advance");
        // source_lines should be in sync
        assert_eq!(
            buffer.source_lines.len(),
            buffer.entries().len(),
            "source_lines and entries must stay in sync"
        );
        let source_sum: usize = buffer.source_lines.iter().sum();
        let expected_jsonl = (new_end + 1).saturating_sub(new_start);
        assert_eq!(
            source_sum, expected_jsonl,
            "source_lines sum ({}) should equal JSONL window size ({})",
            source_sum, expected_jsonl
        );

        // If eviction happened, has_older should still report correctly
        if new_start > 0 {
            assert!(
                buffer.has_older(),
                "should still report has_older after eviction"
            );
        }
    }

    #[test]
    fn test_empty_parse_advances_window() {
        let mut file = NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(file, "{}", user_entry(&format!("line {}", i))).unwrap();
        }
        file.flush().unwrap();

        let mut buffer = EntryBuffer::new(5);
        buffer.load_file(file.path()).unwrap();

        assert!(buffer.has_older());
        let (initial_start, _) = buffer.window_position();

        // Manually create a pending load that will parse an empty range
        // We'll simulate this by requesting a load and providing an empty result
        buffer.clear_rate_limit();
        if let Some((_path, _start, _end)) = buffer.request_load_older(3) {
            // Provide empty parse result
            let empty_result = Ok(ParseResult {
                entries: Vec::new(),
                errors: Vec::new(),
                bytes_read: 0,
            });
            buffer.receive_loaded(empty_result, 80, false, false);
        }

        let (new_start, _) = buffer.window_position();
        // Window should have advanced even with empty result
        assert!(
            new_start < initial_start,
            "window_start should advance on empty parse (was {}, now {})",
            initial_start,
            new_start
        );
    }

    /// Simulate the EXACT handler behavior for PageUp scrolling through a large file.
    /// Now that buffer and render share the same calculate_entry_lines, scroll_delta
    /// matches render's line counting exactly.
    #[test]
    fn test_pageup_simulation_reaches_top() {
        // Test with both short and long text
        for long_text in [false, true] {
            // Build a file with 400 JSONL lines (mix of tool pairs and messages)
            let (file, total_jsonl) = build_tool_file_ex(300, long_text);
            assert!(total_jsonl > 100, "need file larger than buffer capacity");

            let capacity = 100;
            let mut buffer = EntryBuffer::new(capacity);
            buffer.load_file(file.path()).unwrap();

            let content_width = 190; // Typical terminal width after borders/padding
            let viewport_height = 46;
            let threshold = viewport_height / 2; // 23
            let show_thinking = false;

            // Test both expand_tools=false and expand_tools=true
            for expand_tools in [false, true] {
                // Reload for each test
                buffer.load_file(file.path()).unwrap();

                let render_total = |buf: &EntryBuffer| -> usize {
                    buf.entries()
                        .iter()
                        .map(|e| {
                            calculate_entry_lines(e, content_width, show_thinking, expand_tools)
                        })
                        .sum()
                };

                let mut total_lines = render_total(&buffer);
                let mut scroll_offset: usize = total_lines.saturating_sub(viewport_height);

                eprintln!(
                    "\n=== long_text={}, expand_tools={} ===",
                    long_text, expand_tools
                );
                eprintln!(
                    "Initial: total_jsonl={}, total_lines={}, scroll_offset={}, window=({}, {})",
                    total_jsonl,
                    total_lines,
                    scroll_offset,
                    buffer.window_position().0,
                    buffer.window_position().1
                );

                let max_presses = 200;
                let mut prev_window_start = buffer.window_position().0;
                let mut stall_count = 0;

                for press in 0..max_presses {
                    // === scroll_up (matches handler) ===
                    scroll_offset = scroll_offset.saturating_sub(viewport_height);

                    // === check_and_trigger_load (matches handler) ===
                    // Single-direction: check near_top ONCE, load up to 5 batches.
                    let near_top = scroll_offset < threshold && buffer.has_older();
                    for _ in 0..5 {
                        if !near_top || !buffer.has_older() {
                            break;
                        }
                        buffer.clear_rate_limit();
                        if let Some((path, start, end)) = buffer.request_load_older(40) {
                            let result = parse_jsonl_range(&path, start, end);
                            let scroll_delta = buffer.receive_loaded(
                                result,
                                content_width,
                                show_thinking,
                                expand_tools,
                            );
                            if scroll_delta != 0 {
                                scroll_offset =
                                    (scroll_offset as isize + scroll_delta).max(0) as usize;
                            }
                        } else {
                            break;
                        }
                    }

                    // === Simulate render clamping (using RENDER's total_lines) ===
                    total_lines = render_total(&buffer);
                    let max_scroll = total_lines.saturating_sub(viewport_height);
                    if scroll_offset > max_scroll {
                        eprintln!(
                            "  CLAMP at press {}: scroll_offset {} -> {} (max_scroll={}, total_lines={})",
                            press, scroll_offset, max_scroll, max_scroll, total_lines
                        );
                    }
                    scroll_offset = scroll_offset.min(max_scroll);

                    let (win_start, win_end) = buffer.window_position();

                    // Check for reaching the top
                    if win_start == 0 && !buffer.has_older() {
                        eprintln!("  Reached top at press {}", press);
                        break;
                    }

                    // Check for stall
                    if win_start == prev_window_start && scroll_offset == 0 && buffer.has_older() {
                        stall_count += 1;
                        if stall_count > 5 {
                            panic!(
                                "Scrolling stalled with expand_tools={}! window=({}, {}), \
                            has_older={}, total_lines={}, {} JSONL lines unreachable.",
                                expand_tools,
                                win_start,
                                win_end,
                                buffer.has_older(),
                                total_lines,
                                win_start
                            );
                        }
                    } else {
                        stall_count = 0;
                    }

                    prev_window_start = win_start;

                    if press % 20 == 0 {
                        eprintln!(
                            "  Press {}: offset={}, window=({}, {}), lines={}, has_older={}",
                            press,
                            scroll_offset,
                            win_start,
                            win_end,
                            total_lines,
                            buffer.has_older()
                        );
                    }
                }

                let (final_start, _) = buffer.window_position();
                assert_eq!(
                    final_start, 0,
                    "long_text={}, expand_tools={}: Should reach top, but stopped at JSONL line {}",
                    long_text, expand_tools, final_start
                );
            }
        } // end for long_text
    }

    #[test]
    fn test_full_traversal_with_tools() {
        // Build a file large enough to exceed buffer
        let (file, total_jsonl) = build_tool_file(30);
        assert!(total_jsonl > 10, "need enough lines to exceed buffer");

        let mut buffer = EntryBuffer::new(10);
        buffer.load_file(file.path()).unwrap();

        // Start at tail - load older until we reach the beginning
        let mut iterations = 0;
        while buffer.has_older() && iterations < 50 {
            buffer.clear_rate_limit();
            if let Some((path, start, end)) = buffer.request_load_older(5) {
                let result = parse_jsonl_range(&path, start, end);
                buffer.receive_loaded(result, 80, false, false);
            } else {
                break;
            }
            iterations += 1;

            // Verify invariant: source_lines stays in sync
            assert_eq!(buffer.source_lines.len(), buffer.entries().len());
        }

        let (start_pos, _) = buffer.window_position();
        assert_eq!(start_pos, 0, "should reach file start");

        // Now traverse forward to the end
        iterations = 0;
        while buffer.has_newer() && iterations < 50 {
            buffer.clear_rate_limit();
            if let Some((path, start, end)) = buffer.request_load_newer(5) {
                let result = parse_jsonl_range(&path, start, end);
                buffer.receive_loaded(result, 80, false, false);
            } else {
                break;
            }
            iterations += 1;

            assert_eq!(buffer.source_lines.len(), buffer.entries().len());
        }

        let (_, end_pos) = buffer.window_position();
        assert_eq!(
            end_pos,
            total_jsonl - 1,
            "should reach file end (expected {}, got {})",
            total_jsonl - 1,
            end_pos
        );
    }
}
