use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct AppLayout {
    pub header: Rect,
    pub projects: Rect,
    pub sessions: Rect,
    pub conversation: Rect,
    pub status_bar: Rect,
}

impl AppLayout {
    pub fn new(area: Rect) -> Self {
        // Vertical split: header, main content, status bar
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Header
                Constraint::Min(10),    // Main content
                Constraint::Length(1),  // Status bar
            ])
            .split(area);

        let header = vertical[0];
        let main = vertical[1];
        let status_bar = vertical[2];

        // Horizontal split for main content: projects, sessions, conversation
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15), // Projects
                Constraint::Percentage(20), // Sessions
                Constraint::Percentage(65), // Conversation
            ])
            .split(main);

        Self {
            header,
            projects: horizontal[0],
            sessions: horizontal[1],
            conversation: horizontal[2],
            status_bar,
        }
    }
}
