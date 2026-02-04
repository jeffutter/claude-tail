use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
        Widget,
    },
};
use unicode_width::UnicodeWidthStr;

use super::styles::Theme;
use crate::logs::{DisplayEntry, ToolCallResult};

pub struct ConversationView<'a> {
    entries: &'a [DisplayEntry],
    focused: bool,
    theme: &'a Theme,
    show_thinking: bool,
    expand_tools: bool,
}

impl<'a> ConversationView<'a> {
    pub fn new(
        entries: &'a [DisplayEntry],
        focused: bool,
        theme: &'a Theme,
        show_thinking: bool,
        expand_tools: bool,
    ) -> Self {
        Self {
            entries,
            focused,
            theme,
            show_thinking,
            expand_tools,
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

    fn render_entries(&self, width: usize) -> Vec<Line<'a>> {
        let mut lines = Vec::new();
        let content_width = width.saturating_sub(4); // Account for borders and padding

        for entry in self.entries {
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

        lines
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

        let lines = self.render_entries(padded.width as usize);
        let total_lines = lines.len();

        // Update state with total lines for scrollbar
        state.total_lines = total_lines;

        // Auto-scroll to bottom if follow mode is enabled
        if state.follow_mode && total_lines > inner.height as usize {
            state.scroll_offset = total_lines.saturating_sub(inner.height as usize);
        }

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(inner.height as usize);
        state.scroll_offset = state.scroll_offset.min(max_scroll);

        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(state.scroll_offset)
            .take(inner.height as usize)
            .collect();

        let paragraph = Paragraph::new(Text::from(visible_lines));
        paragraph.render(padded, buf);

        // Render scrollbar
        if total_lines > inner.height as usize {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            let mut scrollbar_state = ScrollbarState::default()
                .content_length(total_lines)
                .position(state.scroll_offset)
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

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    for line in text.lines() {
        if line.width() <= width {
            lines.push(line.to_string());
        } else {
            // Simple word wrapping
            let mut current_line = String::new();
            for word in line.split_whitespace() {
                if current_line.is_empty() {
                    current_line = word.to_string();
                } else if current_line.width() + 1 + word.width() <= width {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    lines.push(current_line);
                    current_line = word.to_string();
                }
            }
            if !current_line.is_empty() {
                lines.push(current_line);
            }
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

pub struct ConversationState {
    pub scroll_offset: usize,
    pub total_lines: usize,
    pub follow_mode: bool,
}

impl ConversationState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            total_lines: 0,
            follow_mode: true, // Start with follow mode enabled
        }
    }

    pub fn scroll_down(&mut self, amount: usize, viewport_height: usize) {
        self.follow_mode = false;
        let max_scroll = self.total_lines.saturating_sub(viewport_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.follow_mode = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
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
}

impl Default for ConversationState {
    fn default() -> Self {
        Self::new()
    }
}
