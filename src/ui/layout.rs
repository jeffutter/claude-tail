use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Minimum width for collapsed columns (just "P" or "S" with borders)
const COLLAPSED_WIDTH: u16 = 3;

/// Padding for expanded columns (border + space on each side)
const COLUMN_PADDING: u16 = 4;

pub struct AppLayout {
    pub header: Rect,
    pub projects: Rect,
    pub sessions: Rect,
    pub conversation: Rect,
    pub status_bar: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPane {
    Projects,
    Sessions,
    Conversation,
}

pub struct LayoutConfig {
    pub focused_pane: FocusedPane,
    pub max_project_width: u16,
    pub max_session_width: u16,
}

impl AppLayout {
    pub fn new(area: Rect, config: LayoutConfig) -> Self {
        // Vertical split: header, main content, status bar
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Header
                Constraint::Min(10),   // Main content
                Constraint::Length(1), // Status bar
            ])
            .split(area);

        let header = vertical[0];
        let main = vertical[1];
        let status_bar = vertical[2];

        // Calculate column widths based on focus
        let (projects_width, sessions_width) = match config.focused_pane {
            FocusedPane::Projects => {
                // Projects expanded, sessions collapsed
                let proj_width = (config.max_project_width + COLUMN_PADDING).min(main.width / 3);
                (proj_width, COLLAPSED_WIDTH)
            }
            FocusedPane::Sessions => {
                // Sessions expanded, projects collapsed
                let sess_width = (config.max_session_width + COLUMN_PADDING).min(main.width / 2);
                (COLLAPSED_WIDTH, sess_width)
            }
            FocusedPane::Conversation => {
                // Both collapsed
                (COLLAPSED_WIDTH, COLLAPSED_WIDTH)
            }
        };

        // Horizontal split for main content: projects, sessions, conversation
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(projects_width),
                Constraint::Length(sessions_width),
                Constraint::Min(20), // Conversation takes remaining space
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
