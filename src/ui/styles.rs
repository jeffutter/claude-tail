use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
    pub border: Style,
    pub border_focused: Style,
    pub title: Style,
    pub title_focused: Style,
    pub selected: Style,
    pub user_message: Style,
    pub user_label: Style,
    pub assistant_text: Style,
    pub assistant_label: Style,
    pub tool_name: Style,
    pub tool_input: Style,
    pub tool_result: Style,
    pub tool_error: Style,
    pub thinking: Style,
    pub thinking_collapsed: Style,
    pub hook_event: Style,
    pub agent_spawn: Style,
    pub status_bar: Style,
    pub key_hint: Style,
    pub timestamp: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border: Style::default().fg(Color::DarkGray),
            border_focused: Style::default().fg(Color::Cyan),
            title: Style::default().fg(Color::White),
            title_focused: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            selected: Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
            user_message: Style::default().fg(Color::White),
            user_label: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            assistant_text: Style::default().fg(Color::White),
            assistant_label: Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            tool_name: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            tool_input: Style::default().fg(Color::Gray),
            tool_result: Style::default().fg(Color::Cyan),
            tool_error: Style::default().fg(Color::Red),
            thinking: Style::default().fg(Color::DarkGray),
            thinking_collapsed: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            hook_event: Style::default().fg(Color::Blue),
            agent_spawn: Style::default().fg(Color::LightMagenta),
            status_bar: Style::default().bg(Color::DarkGray).fg(Color::White),
            key_hint: Style::default().fg(Color::Cyan),
            timestamp: Style::default().fg(Color::DarkGray),
        }
    }
}

impl Theme {
    pub fn new() -> Self {
        Self::default()
    }
}
