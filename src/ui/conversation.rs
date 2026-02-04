use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::styles::Theme;
use crate::logs::DisplayEntry;

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
                DisplayEntry::ToolCall { name, input, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("Tool: ", self.theme.tool_name),
                        Span::styled(name.clone(), self.theme.tool_name),
                    ]));
                    if self.expand_tools && !input.is_empty() {
                        for line in wrap_text(input, content_width) {
                            lines.push(Line::from(Span::styled(
                                format!("  {}", line),
                                self.theme.tool_input,
                            )));
                        }
                    }
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
                    lines.push(Line::from(Span::styled(
                        format!("[{}]", label),
                        style,
                    )));
                    if self.expand_tools && !content.is_empty() {
                        // Truncate very long results
                        let display_content = if content.len() > 500 {
                            format!("{}...", &content[..500])
                        } else {
                            content.clone()
                        };
                        for line in wrap_text(&display_content, content_width) {
                            lines.push(Line::from(Span::styled(
                                format!("  {}", line),
                                style,
                            )));
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
                DisplayEntry::HookEvent { event, details, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("Hook: ", self.theme.hook_event),
                        Span::styled(event.clone(), self.theme.hook_event),
                    ]));
                    if !details.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", details),
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

        let lines = self.render_entries(inner.width as usize);
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
        paragraph.render(inner, buf);

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
