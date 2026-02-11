use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
        Widget,
    },
};
use std::collections::VecDeque;

use super::styles::Theme;
use crate::logs::{DisplayEntry, ToolCallResult};
use crate::text_utils::wrap_text;

/// Calculate how many lines an entry would generate when rendered.
/// This is the SINGLE source of truth for line counting — the instance method
/// and buffer.rs both delegate to this function.
pub fn calculate_entry_lines(
    entry: &DisplayEntry,
    content_width: usize,
    show_thinking: bool,
    expand_tools: bool,
) -> usize {
    match entry {
        DisplayEntry::UserMessage { text, .. } => {
            1 + wrap_text(text, content_width).len() + 1 // header + wrapped lines + blank
        }
        DisplayEntry::AssistantText { text, .. } => {
            1 + wrap_text(text, content_width).len() + 1 // header + wrapped lines + blank
        }
        DisplayEntry::ToolCall {
            name,
            input,
            result,
            ..
        } => {
            let parsed: Option<serde_json::Value> = serde_json::from_str(input).ok();

            // Tool body lines — matches render_tool_call per-tool logic
            let mut count = match name.as_str() {
                "Bash" => {
                    let mut c = 1; // header
                    if expand_tools
                        && let Some(command) = parsed
                            .as_ref()
                            .and_then(|v| v.get("command"))
                            .and_then(|v| v.as_str())
                        && !command.is_empty()
                    {
                        c += wrap_text(command, content_width.saturating_sub(2)).len();
                    }
                    c
                }
                "Read" => {
                    let mut c = 1; // header
                    if expand_tools {
                        let offset = parsed
                            .as_ref()
                            .and_then(|v| v.get("offset"))
                            .and_then(|v| v.as_u64());
                        let limit = parsed
                            .as_ref()
                            .and_then(|v| v.get("limit"))
                            .and_then(|v| v.as_u64());
                        if offset.is_some() || limit.is_some() {
                            c += 1; // range line
                        }
                    }
                    c
                }
                "Write" => {
                    let mut c = 1; // header
                    if expand_tools
                        && let Some(content) = parsed
                            .as_ref()
                            .and_then(|v| v.get("content"))
                            .and_then(|v| v.as_str())
                    {
                        let line_count = content.lines().count();
                        if line_count > 0 {
                            c += line_count.min(5);
                            if line_count > 5 {
                                c += 1; // "more lines" indicator
                            }
                        }
                    }
                    c
                }
                "Edit" => {
                    let mut c = 1; // header
                    if expand_tools {
                        if let Some(old_string) = parsed
                            .as_ref()
                            .and_then(|v| v.get("old_string"))
                            .and_then(|v| v.as_str())
                            && !old_string.is_empty()
                        {
                            c += 1; // "- old:" label
                            c += old_string.lines().count().min(3);
                            if old_string.lines().count() > 3 {
                                c += 1; // "more lines" indicator
                            }
                        }
                        if let Some(new_string) = parsed
                            .as_ref()
                            .and_then(|v| v.get("new_string"))
                            .and_then(|v| v.as_str())
                            && !new_string.is_empty()
                        {
                            c += 1; // "+ new:" label
                            c += new_string.lines().count().min(3);
                            if new_string.lines().count() > 3 {
                                c += 1; // "more lines" indicator
                            }
                        }
                    }
                    c
                }
                "Grep" => {
                    let mut c = 1; // header
                    if expand_tools {
                        let has_path = parsed
                            .as_ref()
                            .and_then(|v| v.get("path"))
                            .and_then(|v| v.as_str())
                            .is_some();
                        let has_glob = parsed
                            .as_ref()
                            .and_then(|v| v.get("glob"))
                            .and_then(|v| v.as_str())
                            .is_some();
                        if has_path || has_glob {
                            c += 1; // details line
                        }
                    }
                    c
                }
                "Glob" => {
                    let mut c = 1; // header
                    if expand_tools
                        && parsed
                            .as_ref()
                            .and_then(|v| v.get("path"))
                            .and_then(|v| v.as_str())
                            .is_some()
                    {
                        c += 1; // path line
                    }
                    c
                }
                "Task" => {
                    let mut c = 1; // header
                    if expand_tools
                        && let Some(prompt) = parsed
                            .as_ref()
                            .and_then(|v| v.get("prompt"))
                            .and_then(|v| v.as_str())
                        && !prompt.is_empty()
                    {
                        let display_prompt = if prompt.len() > 300 {
                            &prompt[..300]
                        } else {
                            prompt
                        };
                        c += wrap_text(display_prompt, content_width.saturating_sub(2)).len();
                    }
                    c
                }
                "TodoWrite" => {
                    let mut c = 1; // header
                    if expand_tools
                        && let Some(todos) = parsed
                            .as_ref()
                            .and_then(|v| v.get("todos"))
                            .and_then(|v| v.as_array())
                    {
                        c += todos.len(); // one line per todo item
                    }
                    c
                }
                _ => {
                    let mut c = 1; // header
                    if expand_tools && !input.is_empty() {
                        c += wrap_text(input, content_width).len();
                    }
                    c
                }
            };

            // Inline result lines — matches calculate_inline_result_lines
            if let Some(res) = result {
                if expand_tools {
                    count += 1; // separator line
                    if res.content.is_empty() {
                        count += 1; // empty result label
                    } else {
                        let display_content = if res.content.len() > 500 {
                            let truncate_at = res
                                .content
                                .char_indices()
                                .take_while(|(i, _)| *i < 500)
                                .last()
                                .map(|(i, c)| i + c.len_utf8())
                                .unwrap_or(0);
                            &res.content[..truncate_at]
                        } else {
                            res.content.as_str()
                        };
                        count += wrap_text(display_content, content_width.saturating_sub(2)).len();
                    }
                } else {
                    count += 1; // collapsed result indicator
                }
            }

            count + 1 // blank line
        }
        DisplayEntry::ToolResult {
            content,
            is_error: _,
            ..
        } => {
            let mut count = 1; // label line
            if expand_tools && !content.is_empty() {
                let display_content = if content.len() > 500 {
                    let truncate_at = content
                        .char_indices()
                        .take_while(|(i, _)| *i < 500)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(0);
                    &content[..truncate_at]
                } else {
                    content.as_str()
                };
                count += wrap_text(display_content, content_width).len();
            }
            count + 1 // blank line
        }
        DisplayEntry::Thinking { text, .. } => {
            if show_thinking {
                1 + wrap_text(text, content_width).len() + 1 // header + wrapped lines + blank
            } else {
                1 // collapsed indicator
            }
        }
        DisplayEntry::HookEvent { command, .. } => {
            let mut count = 1; // header
            if expand_tools && command.as_ref().is_some_and(|cmd| cmd != "callback") {
                count += 1; // command line
            }
            count + 1 // blank line
        }
        DisplayEntry::AgentSpawn { description, .. } => {
            let mut count = 1; // header
            if !description.is_empty() {
                count += 1; // description line
            }
            count + 1 // blank line
        }
    }
}

pub struct ConversationView<'a> {
    entries: &'a VecDeque<DisplayEntry>,
    focused: bool,
    theme: &'a Theme,
    show_thinking: bool,
    expand_tools: bool,
    is_loading: bool,
    /// Total JSONL lines in file (for approximate scrollbar)
    total_file_lines: usize,
    /// Buffer window position (start_line, end_line)
    window_position: (usize, usize),
}

impl<'a> ConversationView<'a> {
    pub fn new(
        entries: &'a VecDeque<DisplayEntry>,
        focused: bool,
        theme: &'a Theme,
        show_thinking: bool,
        expand_tools: bool,
        is_loading: bool,
        total_file_lines: usize,
        window_position: (usize, usize),
    ) -> Self {
        Self {
            entries,
            focused,
            theme,
            show_thinking,
            expand_tools,
            is_loading,
            total_file_lines,
            window_position,
        }
    }

    fn render_tool_call(
        &self,
        lines: &mut Vec<Line<'a>>,
        name: &str,
        input: &str,
        result: Option<&ToolCallResult>,
        content_width: usize,
    ) {
        // Parse the JSON input to extract relevant fields
        let parsed: Option<serde_json::Value> = serde_json::from_str(input).ok();

        match name {
            "Bash" => self.render_bash_tool(lines, parsed.as_ref(), content_width),
            "Read" => self.render_read_tool(lines, parsed.as_ref(), content_width),
            "Write" => self.render_write_tool(lines, parsed.as_ref(), content_width),
            "Edit" => self.render_edit_tool(lines, parsed.as_ref(), content_width),
            "Grep" => self.render_grep_tool(lines, parsed.as_ref(), content_width),
            "Glob" => self.render_glob_tool(lines, parsed.as_ref(), content_width),
            "Task" => self.render_task_tool(lines, parsed.as_ref(), content_width),
            "TodoWrite" => self.render_todowrite_tool(lines, parsed.as_ref(), content_width),
            _ => self.render_generic_tool(lines, name, input, content_width),
        }

        // Render inline result if present
        if let Some(res) = result {
            self.render_inline_result(lines, res, content_width);
        }
    }

    fn render_inline_result(
        &self,
        lines: &mut Vec<Line<'a>>,
        result: &ToolCallResult,
        content_width: usize,
    ) {
        if !self.expand_tools {
            // Show collapsed indicator
            let style = if result.is_error {
                self.theme.tool_error
            } else {
                self.theme.tool_result
            };
            let label = if result.is_error { "Error" } else { "OK" };
            lines.push(Line::from(Span::styled(format!("  → [{}]", label), style)));
            return;
        }

        // Separator line
        lines.push(Line::from(Span::styled(
            format!("  {}", "─".repeat(content_width.saturating_sub(4).min(40))),
            self.theme.border,
        )));

        let style = if result.is_error {
            self.theme.tool_error
        } else {
            self.theme.tool_result
        };

        if result.content.is_empty() {
            let label = if result.is_error {
                "[Error: no output]"
            } else {
                "[OK]"
            };
            lines.push(Line::from(Span::styled(format!("  {}", label), style)));
        } else {
            // Truncate very long results (respecting char boundaries)
            let display_content = if result.content.len() > 500 {
                let truncate_at = result
                    .content
                    .char_indices()
                    .take_while(|(i, _)| *i < 500)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}...", &result.content[..truncate_at])
            } else {
                result.content.clone()
            };
            for line in wrap_text(&display_content, content_width.saturating_sub(2)) {
                lines.push(Line::from(Span::styled(format!("  {}", line), style)));
            }
        }
    }

    fn render_bash_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        content_width: usize,
    ) {
        let command = parsed
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = parsed
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Header with description if available
        if let Some(desc) = description {
            lines.push(Line::from(vec![
                Span::styled("$ ", self.theme.tool_name),
                Span::styled(desc, self.theme.tool_name),
            ]));
        } else {
            lines.push(Line::from(Span::styled("$ Bash", self.theme.tool_name)));
        }

        // Command with syntax highlighting style
        if self.expand_tools && !command.is_empty() {
            for line in wrap_text(&command, content_width.saturating_sub(2)) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    self.theme.tool_input,
                )));
            }
        }
    }

    fn render_read_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        _content_width: usize,
    ) {
        let file_path = parsed
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>")
            .to_string();

        // Abbreviate the path for display
        let display_path = abbreviate_path(&file_path);

        lines.push(Line::from(vec![
            Span::styled("Read: ", self.theme.tool_name),
            Span::styled(display_path, self.theme.tool_input),
        ]));

        // Show offset/limit if present
        if self.expand_tools {
            let offset = parsed
                .and_then(|v| v.get("offset"))
                .and_then(|v| v.as_u64());
            let limit = parsed.and_then(|v| v.get("limit")).and_then(|v| v.as_u64());

            if offset.is_some() || limit.is_some() {
                let mut range_parts = Vec::new();
                if let Some(off) = offset {
                    range_parts.push(format!("offset: {}", off));
                }
                if let Some(lim) = limit {
                    range_parts.push(format!("limit: {}", lim));
                }
                lines.push(Line::from(Span::styled(
                    format!("  [{}]", range_parts.join(", ")),
                    self.theme.tool_input,
                )));
            }
        }
    }

    fn render_write_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        content_width: usize,
    ) {
        let file_path = parsed
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>")
            .to_string();
        let content = parsed
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Abbreviate the path for display
        let display_path = abbreviate_path(&file_path);

        // Count lines in content
        let line_count = content.lines().count();

        lines.push(Line::from(vec![
            Span::styled("Write: ", self.theme.tool_name),
            Span::styled(display_path, self.theme.tool_input),
            Span::styled(
                format!(" ({} lines)", line_count),
                self.theme.thinking_collapsed,
            ),
        ]));

        // Show preview of content
        if self.expand_tools && !content.is_empty() {
            let preview_lines: Vec<&str> = content.lines().take(5).collect();
            for line in &preview_lines {
                let truncated = if line.len() > content_width.saturating_sub(4) {
                    format!("{}…", &line[..content_width.saturating_sub(5)])
                } else {
                    line.to_string()
                };
                lines.push(Line::from(Span::styled(
                    format!("  │ {}", truncated),
                    self.theme.tool_input,
                )));
            }
            if line_count > 5 {
                lines.push(Line::from(Span::styled(
                    format!("  │ ... ({} more lines)", line_count - 5),
                    self.theme.thinking_collapsed,
                )));
            }
        }
    }

    fn render_edit_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        content_width: usize,
    ) {
        let file_path = parsed
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>")
            .to_string();
        let old_string = parsed
            .and_then(|v| v.get("old_string"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let new_string = parsed
            .and_then(|v| v.get("new_string"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Abbreviate the path for display
        let display_path = abbreviate_path(&file_path);

        lines.push(Line::from(vec![
            Span::styled("Edit: ", self.theme.tool_name),
            Span::styled(display_path, self.theme.tool_input),
        ]));

        if self.expand_tools {
            // Show old string (what's being replaced)
            if !old_string.is_empty() {
                let old_preview: Vec<&str> = old_string.lines().take(3).collect();
                lines.push(Line::from(Span::styled("  - old:", self.theme.tool_error)));
                for line in old_preview {
                    let truncated = truncate_line(line, content_width.saturating_sub(6));
                    lines.push(Line::from(Span::styled(
                        format!("    {}", truncated),
                        self.theme.tool_error,
                    )));
                }
                if old_string.lines().count() > 3 {
                    lines.push(Line::from(Span::styled(
                        format!("    ... ({} more lines)", old_string.lines().count() - 3),
                        self.theme.thinking_collapsed,
                    )));
                }
            }

            // Show new string (the replacement)
            if !new_string.is_empty() {
                let new_preview: Vec<&str> = new_string.lines().take(3).collect();
                lines.push(Line::from(Span::styled("  + new:", self.theme.tool_result)));
                for line in new_preview {
                    let truncated = truncate_line(line, content_width.saturating_sub(6));
                    lines.push(Line::from(Span::styled(
                        format!("    {}", truncated),
                        self.theme.tool_result,
                    )));
                }
                if new_string.lines().count() > 3 {
                    lines.push(Line::from(Span::styled(
                        format!("    ... ({} more lines)", new_string.lines().count() - 3),
                        self.theme.thinking_collapsed,
                    )));
                }
            }
        }
    }

    fn render_grep_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        _content_width: usize,
    ) {
        let pattern = parsed
            .and_then(|v| v.get("pattern"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let path = parsed
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
            .map(abbreviate_path);
        let glob = parsed
            .and_then(|v| v.get("glob"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        lines.push(Line::from(vec![
            Span::styled("Grep: ", self.theme.tool_name),
            Span::styled(format!("/{}/", pattern), self.theme.tool_input),
        ]));

        if self.expand_tools {
            let mut details = Vec::new();
            if let Some(p) = path {
                details.push(format!("in {}", p));
            }
            if let Some(g) = glob {
                details.push(format!("glob: {}", g));
            }
            if !details.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("  [{}]", details.join(", ")),
                    self.theme.thinking_collapsed,
                )));
            }
        }
    }

    fn render_glob_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        _content_width: usize,
    ) {
        let pattern = parsed
            .and_then(|v| v.get("pattern"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let path = parsed
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
            .map(abbreviate_path);

        lines.push(Line::from(vec![
            Span::styled("Glob: ", self.theme.tool_name),
            Span::styled(pattern, self.theme.tool_input),
        ]));

        if self.expand_tools
            && let Some(p) = path
        {
            lines.push(Line::from(Span::styled(
                format!("  [in {}]", p),
                self.theme.thinking_collapsed,
            )));
        }
    }

    fn render_task_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        content_width: usize,
    ) {
        let description = parsed
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let subagent_type = parsed
            .and_then(|v| v.get("subagent_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let prompt = parsed
            .and_then(|v| v.get("prompt"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Header: Task (subagent_type): description
        lines.push(Line::from(vec![
            Span::styled("Task ", self.theme.agent_spawn),
            Span::styled(
                format!("({})", subagent_type),
                self.theme.thinking_collapsed,
            ),
            Span::styled(": ", self.theme.agent_spawn),
            Span::styled(description, self.theme.tool_input),
        ]));

        // Show prompt when expanded
        if self.expand_tools && !prompt.is_empty() {
            // Truncate long prompts (respecting char boundaries)
            let display_prompt = if prompt.len() > 300 {
                let truncate_at = prompt
                    .char_indices()
                    .take_while(|(i, _)| *i < 300)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}...", &prompt[..truncate_at])
            } else {
                prompt
            };
            for line in wrap_text(&display_prompt, content_width.saturating_sub(2)) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    self.theme.thinking,
                )));
            }
        }
    }

    fn render_todowrite_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        parsed: Option<&serde_json::Value>,
        content_width: usize,
    ) {
        let todos = parsed
            .and_then(|v| v.get("todos"))
            .and_then(|v| v.as_array());

        let total = todos.map(|t| t.len()).unwrap_or(0);

        // Count by status
        let (pending, in_progress, completed) = if let Some(items) = todos {
            items.iter().fold((0, 0, 0), |(p, ip, c), item| {
                match item.get("status").and_then(|s| s.as_str()) {
                    Some("completed") => (p, ip, c + 1),
                    Some("in_progress") => (p, ip + 1, c),
                    _ => (p + 1, ip, c),
                }
            })
        } else {
            (0, 0, 0)
        };

        // Header line with status summary
        let mut header_spans = vec![
            Span::styled("Todo ", self.theme.tool_name),
            Span::styled(format!("({})", total), self.theme.thinking_collapsed),
        ];

        if total > 0 {
            header_spans.push(Span::styled(": ", self.theme.tool_name));
            let mut summary_parts = Vec::new();
            if pending > 0 {
                summary_parts.push(format!("{} pending", pending));
            }
            if in_progress > 0 {
                summary_parts.push(format!("{} in progress", in_progress));
            }
            if completed > 0 {
                summary_parts.push(format!("{} completed", completed));
            }
            header_spans.push(Span::styled(
                summary_parts.join(", "),
                self.theme.thinking_collapsed,
            ));
        }

        lines.push(Line::from(header_spans));

        // Expanded: show each todo item
        if self.expand_tools
            && let Some(items) = todos
        {
            for item in items {
                let status = item
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("pending");
                let content = item.get("content").and_then(|c| c.as_str()).unwrap_or("");

                let (symbol, style) = match status {
                    "completed" => ("✓", self.theme.tool_result),
                    "in_progress" => ("◐", self.theme.tool_name),
                    _ => ("○", self.theme.tool_input),
                };

                let truncated = truncate_line(content, content_width.saturating_sub(6));
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(symbol, style),
                    Span::raw(" "),
                    Span::styled(truncated, style),
                ]));
            }
        }
    }

    fn render_generic_tool(
        &self,
        lines: &mut Vec<Line<'a>>,
        name: &str,
        input: &str,
        content_width: usize,
    ) {
        lines.push(Line::from(vec![
            Span::styled("Tool: ", self.theme.tool_name),
            Span::styled(name.to_string(), self.theme.tool_name),
        ]));
        if self.expand_tools && !input.is_empty() {
            for line in wrap_text(input, content_width) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    self.theme.tool_input,
                )));
            }
        }
    }

    /// Calculate how many lines an entry would generate without actually rendering
    fn calculate_entry_lines(&self, entry: &DisplayEntry, content_width: usize) -> usize {
        // Delegate to the standalone function — single source of truth
        calculate_entry_lines(entry, content_width, self.show_thinking, self.expand_tools)
    }

    /// Calculate total lines that would be rendered for all entries
    pub fn calculate_total_lines(&self, width: usize) -> usize {
        let content_width = width.saturating_sub(4);
        self.entries
            .iter()
            .map(|entry| self.calculate_entry_lines(entry, content_width))
            .sum()
    }

    /// Renders entries visible in the viewport plus a small buffer.
    ///
    /// # Returns
    /// Returns a tuple of:
    /// - Vec<Line>: Rendered lines for visible entries (plus buffer)
    /// - usize: The line number where the returned Vec starts (for skip calculation)
    ///
    /// # Arguments
    /// * `width` - Total content width for text wrapping
    /// * `viewport_start` - First line to include (scroll offset)
    /// * `viewport_height` - Number of lines in the visible viewport
    pub fn render_entries(
        &self,
        width: usize,
        viewport_start: usize,
        viewport_height: usize,
    ) -> (Vec<Line<'a>>, usize) {
        let content_width = width.saturating_sub(4); // Account for borders and padding

        // Single pass: calculate both start positions AND line counts to avoid redundant work
        let entry_info: Vec<(usize, usize)> = {
            let mut info = Vec::with_capacity(self.entries.len());
            let mut current_line = 0;
            for entry in self.entries.iter() {
                let line_count = self.calculate_entry_lines(entry, content_width);
                info.push((current_line, line_count));
                current_line += line_count;
            }
            info
        };

        // Determine which entries intersect with viewport (with buffer)
        let viewport_end = viewport_start + viewport_height;
        let buffer = viewport_height / 4; // Buffer proportional to viewport height
        let render_start = viewport_start.saturating_sub(buffer);
        let render_end = viewport_end + buffer;

        // Find entry range to render using cached line counts
        let first_entry = entry_info
            .iter()
            .position(|(start, count)| start + count > render_start)
            .unwrap_or(0);

        let last_entry = entry_info
            .iter()
            .rposition(|(start, _)| *start < render_end)
            .map(|i| i + 1)
            .unwrap_or(self.entries.len())
            .min(self.entries.len());

        // Render only the visible range
        let mut lines = Vec::new();

        // Track where in the full content our rendered lines start
        let render_offset = if first_entry < entry_info.len() {
            entry_info[first_entry].0 // start line of first rendered entry
        } else {
            0
        };

        for entry in &self.entries.iter().collect::<Vec<_>>()[first_entry..last_entry] {
            match entry {
                DisplayEntry::UserMessage { text, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("User", self.theme.user_label),
                        Span::raw(": "),
                    ]));
                    for line in wrap_text(text, content_width) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", line),
                            self.theme.user_message,
                        )));
                    }
                    lines.push(Line::from(""));
                }
                DisplayEntry::AssistantText { text, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("Assistant", self.theme.assistant_label),
                        Span::raw(": "),
                    ]));
                    for line in wrap_text(text, content_width) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", line),
                            self.theme.assistant_text,
                        )));
                    }
                    lines.push(Line::from(""));
                }
                DisplayEntry::ToolCall {
                    name,
                    input,
                    result,
                    ..
                } => {
                    self.render_tool_call(&mut lines, name, input, result.as_ref(), content_width);
                    lines.push(Line::from(""));
                }
                DisplayEntry::ToolResult {
                    content, is_error, ..
                } => {
                    let (label, style) = if *is_error {
                        ("Error", self.theme.tool_error)
                    } else {
                        ("Result", self.theme.tool_result)
                    };
                    lines.push(Line::from(Span::styled(format!("[{}]", label), style)));
                    if self.expand_tools && !content.is_empty() {
                        // Truncate very long results (respecting char boundaries)
                        let display_content = if content.len() > 500 {
                            let truncate_at = content
                                .char_indices()
                                .take_while(|(i, _)| *i < 500)
                                .last()
                                .map(|(i, c)| i + c.len_utf8())
                                .unwrap_or(0);
                            format!("{}...", &content[..truncate_at])
                        } else {
                            content.clone()
                        };
                        for line in wrap_text(&display_content, content_width) {
                            lines.push(Line::from(Span::styled(format!("  {}", line), style)));
                        }
                    }
                    lines.push(Line::from(""));
                }
                DisplayEntry::Thinking { text, .. } => {
                    if self.show_thinking {
                        lines.push(Line::from(Span::styled(
                            "Thinking:",
                            self.theme.thinking_collapsed,
                        )));
                        for line in wrap_text(text, content_width) {
                            lines.push(Line::from(Span::styled(
                                format!("  {}", line),
                                self.theme.thinking,
                            )));
                        }
                        lines.push(Line::from(""));
                    } else {
                        lines.push(Line::from(Span::styled(
                            "[Thinking collapsed - press 't' to show]",
                            self.theme.thinking_collapsed,
                        )));
                    }
                }
                DisplayEntry::HookEvent {
                    event,
                    hook_name,
                    command,
                    ..
                } => {
                    // Extract tool name from hook_name if present (e.g., "PostToolUse:Read" -> "Read")
                    let tool_info = hook_name
                        .as_ref()
                        .and_then(|name| name.split(':').nth(1).map(|s| s.to_string()));

                    // Build the header line
                    let header = if let Some(tool) = &tool_info {
                        format!("Hook: {} ({})", event, tool)
                    } else {
                        format!("Hook: {}", event)
                    };

                    lines.push(Line::from(Span::styled(header, self.theme.hook_event)));

                    // Show command if expanded and it's a real command (not just "callback")
                    if self.expand_tools
                        && let Some(cmd) = command
                        && cmd != "callback"
                    {
                        // Abbreviate long commands (respecting char boundaries)
                        let display_cmd = if cmd.len() > 60 {
                            let truncate_at = cmd
                                .char_indices()
                                .take_while(|(i, _)| *i < 57)
                                .last()
                                .map(|(i, c)| i + c.len_utf8())
                                .unwrap_or(0);
                            format!("{}...", &cmd[..truncate_at])
                        } else {
                            cmd.clone()
                        };
                        lines.push(Line::from(Span::styled(
                            format!("  → {}", display_cmd),
                            self.theme.hook_event,
                        )));
                    }
                    lines.push(Line::from(""));
                }
                DisplayEntry::AgentSpawn {
                    agent_type,
                    description,
                    ..
                } => {
                    lines.push(Line::from(vec![
                        Span::styled("Agent: ", self.theme.agent_spawn),
                        Span::styled(agent_type.clone(), self.theme.agent_spawn),
                    ]));
                    if !description.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", description),
                            self.theme.agent_spawn,
                        )));
                    }
                    lines.push(Line::from(""));
                }
            }
        }

        (lines, render_offset)
    }
}

impl<'a> StatefulWidget for ConversationView<'a> {
    type State = ConversationState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let (border_style, title_style) = if self.focused {
            (self.theme.border_focused, self.theme.title_focused)
        } else {
            (self.theme.border, self.theme.title)
        };

        let block = Block::default()
            .title(Span::styled(" Conversation ", title_style))
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        block.render(area, buf);

        // Add horizontal padding
        let padded = Rect {
            x: inner.x + 1,
            y: inner.y,
            width: inner.width.saturating_sub(2),
            height: inner.height,
        };

        // Show loading indicator if parsing
        if self.is_loading {
            let loading_text = vec![Line::from(Span::styled(
                "Loading conversation...",
                self.theme.assistant_text,
            ))];
            let paragraph = Paragraph::new(Text::from(loading_text));
            paragraph.render(padded, buf);
            return;
        }

        // Calculate total lines first for follow mode and scrolling
        let total_lines = self.calculate_total_lines(padded.width as usize);

        // Update state with total lines for scrollbar
        state.total_lines = total_lines;

        // Auto-scroll to bottom if follow mode is enabled
        if state.follow_mode && total_lines > inner.height as usize {
            state.scroll_offset = total_lines.saturating_sub(inner.height as usize);
        }

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(inner.height as usize);
        let pre_clamp = state.scroll_offset;
        state.scroll_offset = state.scroll_offset.min(max_scroll);
        if pre_clamp != state.scroll_offset {
            tracing::debug!(
                pre_clamp,
                clamped_to = state.scroll_offset,
                max_scroll,
                total_lines,
                viewport_height = inner.height,
                content_width = padded.width as usize - 4,
                entry_count = self.entries.len(),
                "Render clamped scroll_offset"
            );
        }

        // Render entries in viewport range (with small buffer)
        let (lines, render_offset) = self.render_entries(
            padded.width as usize,
            state.scroll_offset,
            inner.height as usize,
        );

        // Calculate how many lines to skip from the rendered content
        // render_offset is where the rendered lines start in the full content
        let skip_in_rendered = state.scroll_offset.saturating_sub(render_offset);

        // Slice the visible portion from the rendered lines
        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(skip_in_rendered)
            .take(inner.height as usize)
            .collect();

        let paragraph = Paragraph::new(Text::from(visible_lines));
        paragraph.render(padded, buf);

        // Render scrollbar with approximate file position
        if self.total_file_lines > 0 && total_lines > inner.height as usize {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            // Calculate avg for updating the tracked position
            let (win_start, win_end) = self.window_position;
            let buffered_jsonl_lines = (win_end.saturating_sub(win_start)).max(1) as f64;
            let avg_rendered_per_jsonl = total_lines as f64 / buffered_jsonl_lines;

            // Update tracked position. EMA smoothing happens inside, only when state changes.
            state.update_rendered_position(
                (win_start, win_end),
                avg_rendered_per_jsonl,
                self.total_file_lines,
                total_lines,
                inner.height as usize,
            );

            // Estimate total rendered lines in full file using smoothed ratio
            let estimated_total_rendered =
                (self.total_file_lines as f64 * state.smoothed_avg_ratio).max(1.0);

            let mut scrollbar_state = ScrollbarState::default()
                .content_length(estimated_total_rendered as usize)
                .position(state.estimated_rendered_position as usize)
                .viewport_content_length(inner.height as usize);

            scrollbar.render(
                area.inner(ratatui::layout::Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                buf,
                &mut scrollbar_state,
            );
        }
    }
}

/// Truncates a line to fit within a given width, adding ellipsis if needed
fn truncate_line(line: &str, max_width: usize) -> String {
    if line.len() <= max_width {
        line.to_string()
    } else if max_width > 1 {
        format!("{}…", &line[..max_width - 1])
    } else {
        "…".to_string()
    }
}

/// Abbreviates a file path for display (e.g., ~/s/c/project/src/main.rs)
fn abbreviate_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();

    // Replace home directory with ~
    let path = if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    };

    // Abbreviate directory components, keeping the last 2 full
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 3 {
        return path;
    }

    let abbreviated: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            // Keep first (~ or empty for root), last two components, and abbreviate the rest
            if i == 0 || i >= parts.len() - 2 || part.is_empty() {
                part.to_string()
            } else {
                part.chars()
                    .next()
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
        })
        .collect();

    abbreviated.join("/")
}

pub struct ConversationState {
    pub scroll_offset: usize,
    pub total_lines: usize,
    pub follow_mode: bool,
    pub estimated_rendered_position: f64, // Estimated rendered line position in full file
    pending_user_scroll: isize,           // User-intended scroll (not buffer adjustments)
    last_window: (usize, usize),          // Previous (win_start, win_end) to detect buffer shifts
    smoothed_avg_ratio: f64, // Smoothed avg rendered lines per JSONL line (for stable viewport size)
}

impl ConversationState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            total_lines: 0,
            follow_mode: true, // Start with follow mode enabled
            estimated_rendered_position: 0.0,
            pending_user_scroll: 0,
            last_window: (0, 0),
            smoothed_avg_ratio: 1.0,
        }
    }

    pub fn scroll_down(&mut self, amount: usize, viewport_height: usize) {
        self.follow_mode = false;
        let max_scroll = self.total_lines.saturating_sub(viewport_height);
        let old = self.scroll_offset;
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
        self.pending_user_scroll += (self.scroll_offset - old) as isize;
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.follow_mode = false;
        let old = self.scroll_offset;
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        self.pending_user_scroll -= (old - self.scroll_offset) as isize;
    }

    pub fn scroll_to_top(&mut self) {
        self.follow_mode = false;
        self.scroll_offset = 0;
    }

    pub fn scroll_to_bottom(&mut self, viewport_height: usize) {
        self.scroll_offset = self.total_lines.saturating_sub(viewport_height);
        self.follow_mode = true;
    }

    pub fn toggle_follow(&mut self) {
        self.follow_mode = !self.follow_mode;
    }

    /// Update estimated rendered position in full file.
    /// Only recalculates when scroll_offset or window actually changed, to prevent
    /// oscillation from EMA updates on every render frame (the loop redraws every 100ms).
    pub fn update_rendered_position(
        &mut self,
        current_window: (usize, usize),
        avg_rendered_per_jsonl: f64,
        total_file_lines: usize,
        total_rendered_lines: usize,
        viewport_height: usize,
    ) {
        let window_changed = current_window != self.last_window;
        let has_pending = self.pending_user_scroll != 0;

        // Skip update if nothing to do
        if !window_changed && !has_pending {
            self.last_window = current_window;
            return;
        }

        let (win_start, _win_end) = current_window;

        // Update smoothed ratio only when state actually changes (not every frame)
        let alpha = 0.3;
        self.smoothed_avg_ratio =
            alpha * avg_rendered_per_jsonl + (1.0 - alpha) * self.smoothed_avg_ratio;

        let estimated_total_rendered = (total_file_lines as f64 * self.smoothed_avg_ratio).max(1.0);
        let rendered_before_window = (win_start as f64 * self.smoothed_avg_ratio).max(0.0);

        // Snap to accurate boundary positions
        if self.scroll_offset == 0 {
            self.estimated_rendered_position = rendered_before_window;
        } else {
            let max_scroll = total_rendered_lines.saturating_sub(viewport_height);
            if self.scroll_offset >= max_scroll && max_scroll > 0 {
                self.estimated_rendered_position =
                    rendered_before_window + total_rendered_lines as f64;
            } else if has_pending {
                // Apply only the user's intentional scroll, ignoring buffer adjustments.
                // Buffer loads adjust scroll_offset to maintain the same view — they
                // must NOT move the scrollbar position.
                self.estimated_rendered_position += self.pending_user_scroll as f64;

                // Clamp to buffer bounds
                let min_pos = rendered_before_window;
                let max_pos = rendered_before_window + total_rendered_lines as f64;
                self.estimated_rendered_position =
                    self.estimated_rendered_position.max(min_pos).min(max_pos);
            } else if window_changed {
                // Window shifted without user input (e.g. follow mode / file watcher).
                // Recalculate from current position in buffer.
                self.estimated_rendered_position =
                    rendered_before_window + self.scroll_offset as f64;
            }
        }

        // Final clamp to file bounds
        self.estimated_rendered_position = self
            .estimated_rendered_position
            .max(0.0)
            .min(estimated_total_rendered);

        self.pending_user_scroll = 0;
        self.last_window = current_window;
    }

    /// Reset rendered position to a specific value (for jumps to start/end)
    pub fn set_rendered_position(&mut self, position: f64) {
        self.estimated_rendered_position = position;
    }
}

impl Default for ConversationState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::ToolCallResult;
    use std::collections::VecDeque;

    fn make_view<'a>(
        entries: &'a VecDeque<DisplayEntry>,
        show_thinking: bool,
        expand_tools: bool,
    ) -> ConversationView<'a> {
        let theme = Box::leak(Box::new(Theme::default()));
        ConversationView::new(
            entries,
            false,
            theme,
            show_thinking,
            expand_tools,
            false,
            0,
            (0, 0),
        )
    }

    /// For a single entry, render it and count the actual lines produced,
    /// then compare against calculate_entry_lines.
    fn check_entry_line_count(
        entry: &DisplayEntry,
        content_width: usize,
        show_thinking: bool,
        expand_tools: bool,
    ) {
        let entries: VecDeque<DisplayEntry> = vec![entry.clone()].into();
        let view = make_view(&entries, show_thinking, expand_tools);

        let calculated = calculate_entry_lines(entry, content_width, show_thinking, expand_tools);

        // render_entries takes "padded width" = content_width + 4
        let render_width = content_width + 4;
        let (lines, _) = view.render_entries(render_width, 0, calculated + 10);

        assert_eq!(
            calculated,
            lines.len(),
            "Mismatch for {:?} (expand_tools={}, content_width={}): calculate={}, render={}",
            std::mem::discriminant(entry),
            expand_tools,
            content_width,
            calculated,
            lines.len(),
        );
    }

    fn bash_entry(command: &str, result: Option<(&str, bool)>) -> DisplayEntry {
        let input = serde_json::json!({"command": command}).to_string();
        DisplayEntry::ToolCall {
            name: "Bash".to_string(),
            input,
            id: "toolu_test".to_string(),
            timestamp: None,
            result: result.map(|(content, is_error)| ToolCallResult {
                content: content.to_string(),
                is_error,
            }),
        }
    }

    fn read_entry(path: &str, offset: Option<u64>, limit: Option<u64>) -> DisplayEntry {
        let mut input = serde_json::json!({"file_path": path});
        if let Some(o) = offset {
            input["offset"] = serde_json::json!(o);
        }
        if let Some(l) = limit {
            input["limit"] = serde_json::json!(l);
        }
        DisplayEntry::ToolCall {
            name: "Read".to_string(),
            input: input.to_string(),
            id: "toolu_test".to_string(),
            timestamp: None,
            result: None,
        }
    }

    fn write_entry(path: &str, content: &str, result: Option<(&str, bool)>) -> DisplayEntry {
        let input = serde_json::json!({"file_path": path, "content": content}).to_string();
        DisplayEntry::ToolCall {
            name: "Write".to_string(),
            input,
            id: "toolu_test".to_string(),
            timestamp: None,
            result: result.map(|(c, e)| ToolCallResult {
                content: c.to_string(),
                is_error: e,
            }),
        }
    }

    fn edit_entry(old: &str, new: &str, result: Option<(&str, bool)>) -> DisplayEntry {
        let input =
            serde_json::json!({"file_path": "/tmp/test.rs", "old_string": old, "new_string": new})
                .to_string();
        DisplayEntry::ToolCall {
            name: "Edit".to_string(),
            input,
            id: "toolu_test".to_string(),
            timestamp: None,
            result: result.map(|(c, e)| ToolCallResult {
                content: c.to_string(),
                is_error: e,
            }),
        }
    }

    fn grep_entry(pattern: &str, path: Option<&str>, glob: Option<&str>) -> DisplayEntry {
        let mut input = serde_json::json!({"pattern": pattern});
        if let Some(p) = path {
            input["path"] = serde_json::json!(p);
        }
        if let Some(g) = glob {
            input["glob"] = serde_json::json!(g);
        }
        DisplayEntry::ToolCall {
            name: "Grep".to_string(),
            input: input.to_string(),
            id: "toolu_test".to_string(),
            timestamp: None,
            result: None,
        }
    }

    fn glob_entry(pattern: &str, path: Option<&str>) -> DisplayEntry {
        let mut input = serde_json::json!({"pattern": pattern});
        if let Some(p) = path {
            input["path"] = serde_json::json!(p);
        }
        DisplayEntry::ToolCall {
            name: "Glob".to_string(),
            input: input.to_string(),
            id: "toolu_test".to_string(),
            timestamp: None,
            result: None,
        }
    }

    fn task_entry(prompt: &str, result: Option<(&str, bool)>) -> DisplayEntry {
        let input = serde_json::json!({"description": "test task", "subagent_type": "Bash", "prompt": prompt}).to_string();
        DisplayEntry::ToolCall {
            name: "Task".to_string(),
            input,
            id: "toolu_test".to_string(),
            timestamp: None,
            result: result.map(|(c, e)| ToolCallResult {
                content: c.to_string(),
                is_error: e,
            }),
        }
    }

    fn todowrite_entry(count: usize) -> DisplayEntry {
        let todos: Vec<serde_json::Value> = (0..count)
            .map(
                |i| serde_json::json!({"content": format!("todo item {}", i), "status": "pending"}),
            )
            .collect();
        let input = serde_json::json!({"todos": todos}).to_string();
        DisplayEntry::ToolCall {
            name: "TodoWrite".to_string(),
            input,
            id: "toolu_test".to_string(),
            timestamp: None,
            result: None,
        }
    }

    fn generic_tool_entry(name: &str, input: &str, result: Option<(&str, bool)>) -> DisplayEntry {
        DisplayEntry::ToolCall {
            name: name.to_string(),
            input: input.to_string(),
            id: "toolu_test".to_string(),
            timestamp: None,
            result: result.map(|(c, e)| ToolCallResult {
                content: c.to_string(),
                is_error: e,
            }),
        }
    }

    /// Test that calculate_entry_lines matches actual render output for every entry type.
    #[test]
    fn test_calculate_matches_render_for_all_entry_types() {
        let long_text = "a".repeat(500);
        let very_long_text = "word ".repeat(200); // 1000 chars
        let multiline_text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";

        let entries: Vec<DisplayEntry> = vec![
            // UserMessage
            DisplayEntry::UserMessage {
                text: "short".to_string(),
                timestamp: None,
            },
            DisplayEntry::UserMessage {
                text: long_text.clone(),
                timestamp: None,
            },
            // AssistantText
            DisplayEntry::AssistantText {
                text: "short reply".to_string(),
                timestamp: None,
            },
            DisplayEntry::AssistantText {
                text: very_long_text.clone(),
                timestamp: None,
            },
            // Bash
            bash_entry("ls -la", None),
            bash_entry("ls -la", Some(("output here", false))),
            bash_entry("ls -la", Some((&long_text, false))),
            bash_entry(&very_long_text, Some(("ok", false))),
            // Read
            read_entry("/tmp/test.rs", None, None),
            read_entry("/tmp/test.rs", Some(10), Some(50)),
            // Write
            write_entry("/tmp/test.rs", "short", None),
            write_entry("/tmp/test.rs", multiline_text, Some(("ok", false))),
            // Edit
            edit_entry("old code", "new code", None),
            edit_entry(multiline_text, multiline_text, Some(("ok", false))),
            // Grep
            grep_entry("pattern", None, None),
            grep_entry("pattern", Some("/tmp"), Some("*.rs")),
            // Glob
            glob_entry("**/*.rs", None),
            glob_entry("**/*.rs", Some("/tmp")),
            // Task
            task_entry("short prompt", None),
            task_entry(&very_long_text, Some(("result", false))),
            // TodoWrite
            todowrite_entry(0),
            todowrite_entry(5),
            // Generic tool
            generic_tool_entry("CustomTool", r#"{"key": "value"}"#, None),
            generic_tool_entry("CustomTool", &very_long_text, Some((&long_text, false))),
            // ToolResult (standalone)
            DisplayEntry::ToolResult {
                tool_use_id: "id".to_string(),
                content: "result".to_string(),
                is_error: false,
                timestamp: None,
            },
            DisplayEntry::ToolResult {
                tool_use_id: "id".to_string(),
                content: long_text.clone(),
                is_error: true,
                timestamp: None,
            },
            // Thinking
            DisplayEntry::Thinking {
                text: "thinking...".to_string(),
                collapsed: false,
                timestamp: None,
            },
            DisplayEntry::Thinking {
                text: very_long_text.clone(),
                collapsed: false,
                timestamp: None,
            },
            // HookEvent
            DisplayEntry::HookEvent {
                event: "PreToolUse".to_string(),
                hook_name: Some("PreToolUse:Read".to_string()),
                command: Some("my-hook.sh".to_string()),
                timestamp: None,
            },
            DisplayEntry::HookEvent {
                event: "PostToolUse".to_string(),
                hook_name: None,
                command: Some("callback".to_string()),
                timestamp: None,
            },
            // AgentSpawn
            DisplayEntry::AgentSpawn {
                agent_type: "Bash".to_string(),
                description: "test agent".to_string(),
                timestamp: None,
            },
            DisplayEntry::AgentSpawn {
                agent_type: "Bash".to_string(),
                description: String::new(),
                timestamp: None,
            },
        ];

        for width in [80, 120, 200] {
            for expand_tools in [false, true] {
                for show_thinking in [false, true] {
                    for entry in &entries {
                        check_entry_line_count(entry, width, show_thinking, expand_tools);
                    }
                }
            }
        }
    }

    /// Test that rendering a full set of entries produces exactly
    /// calculate_total_lines worth of lines.
    #[test]
    fn test_total_rendered_lines_matches_calculate() {
        let entries: VecDeque<DisplayEntry> = vec![
            DisplayEntry::UserMessage {
                text: "hello".to_string(),
                timestamp: None,
            },
            bash_entry("ls -la", Some(("file1\nfile2\nfile3", false))),
            DisplayEntry::AssistantText {
                text: "I found those files.".to_string(),
                timestamp: None,
            },
            edit_entry("old\ncode\nhere\nmore", "new\ncode", Some(("ok", false))),
            DisplayEntry::Thinking {
                text: "Let me think about this carefully...".to_string(),
                collapsed: false,
                timestamp: None,
            },
            task_entry(
                "Search the codebase for all instances of the function",
                Some(("found 5 matches", false)),
            ),
            DisplayEntry::HookEvent {
                event: "PostToolUse".to_string(),
                hook_name: Some("PostToolUse:Bash".to_string()),
                command: Some("validate.sh".to_string()),
                timestamp: None,
            },
        ]
        .into();

        for width in [80, 120, 200] {
            for expand_tools in [false, true] {
                let view = make_view(&entries, true, expand_tools);
                let render_width = width + 4; // render_entries subtracts 4
                let calculated = view.calculate_total_lines(render_width);
                let (lines, _) = view.render_entries(render_width, 0, calculated + 100);
                assert_eq!(
                    calculated,
                    lines.len(),
                    "Total lines mismatch (expand_tools={}, width={}): calculate={}, render={}",
                    expand_tools,
                    width,
                    calculated,
                    lines.len(),
                );
            }
        }
    }
}
