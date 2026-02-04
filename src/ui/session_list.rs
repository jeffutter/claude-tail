use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::styles::Theme;
use crate::logs::Session;

pub struct SessionList<'a> {
    sessions: &'a [Session],
    focused: bool,
    collapsed: bool,
    theme: &'a Theme,
}

impl<'a> SessionList<'a> {
    pub fn new(sessions: &'a [Session], focused: bool, collapsed: bool, theme: &'a Theme) -> Self {
        Self {
            sessions,
            focused,
            collapsed,
            theme,
        }
    }

    /// Calculate the maximum display width needed for the session list
    pub fn max_content_width(sessions: &[Session]) -> u16 {
        sessions
            .iter()
            .map(|s| s.display_name().width() + 2) // +2 for "> " prefix
            .max()
            .unwrap_or(15) as u16
    }
}

impl<'a> StatefulWidget for SessionList<'a> {
    type State = ListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let (border_style, title_style) = if self.focused {
            (self.theme.border_focused, self.theme.title_focused)
        } else {
            (self.theme.border, self.theme.title)
        };

        if self.collapsed {
            // Render collapsed view - just "S" with borders
            let block = Block::default()
                .title(Span::styled("S", title_style))
                .borders(Borders::ALL)
                .border_style(border_style);
            block.render(area, buf);
            return;
        }

        let items: Vec<ListItem> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, session)| {
                let is_selected = state.selected() == Some(i);
                let prefix = if is_selected { "> " } else { "  " };
                let style = if is_selected {
                    self.theme.selected
                } else {
                    ratatui::style::Style::default()
                };

                ListItem::new(Line::from(vec![Span::styled(
                    format!("{}{}", prefix, session.display_name()),
                    style,
                )]))
            })
            .collect();

        let block = Block::default()
            .title(Span::styled(" Sessions ", title_style))
            .borders(Borders::ALL)
            .border_style(border_style);

        let list = List::new(items)
            .block(block)
            .highlight_style(self.theme.selected.add_modifier(Modifier::BOLD));

        StatefulWidget::render(list, area, buf, state);
    }
}

pub struct SessionListState {
    pub list_state: ListState,
}

impl SessionListState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self { list_state }
    }

    pub fn selected(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn select(&mut self, index: Option<usize>) {
        self.list_state.select(index);
    }

    pub fn next(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn previous(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn first(&mut self) {
        self.list_state.select(Some(0));
    }

    pub fn last(&mut self, len: usize) {
        if len > 0 {
            self.list_state.select(Some(len - 1));
        }
    }
}

impl Default for SessionListState {
    fn default() -> Self {
        Self::new()
    }
}
