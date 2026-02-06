use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::styles::Theme;
use crate::logs::Agent;

pub struct AgentList<'a> {
    agents: &'a [Agent],
    focused: bool,
    collapsed: bool,
    theme: &'a Theme,
}

impl<'a> AgentList<'a> {
    pub fn new(agents: &'a [Agent], focused: bool, collapsed: bool, theme: &'a Theme) -> Self {
        Self {
            agents,
            focused,
            collapsed,
            theme,
        }
    }

    /// Calculate the maximum display width needed for the agent list
    pub fn max_content_width(agents: &[Agent]) -> u16 {
        agents
            .iter()
            .map(|a| a.display_name_with_timestamp().width() + 2) // +2 for "> " prefix
            .max()
            .unwrap_or(10) as u16
    }
}

impl<'a> StatefulWidget for AgentList<'a> {
    type State = ListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let (border_style, title_style) = if self.focused {
            (self.theme.border_focused, self.theme.title_focused)
        } else {
            (self.theme.border, self.theme.title)
        };

        if self.collapsed {
            // Render collapsed view - just "A" with borders
            let block = Block::default()
                .title(Span::styled("A", title_style))
                .borders(Borders::ALL)
                .border_style(border_style);
            block.render(area, buf);
            return;
        }

        // Calculate available width for right-aligning timestamps
        let available_width = area.width.saturating_sub(2); // Subtract borders

        let items: Vec<ListItem> = self
            .agents
            .iter()
            .enumerate()
            .map(|(i, agent)| {
                let is_selected = state.selected() == Some(i);
                let prefix = if is_selected { "> " } else { "  " };

                // Use different style for main agent
                let label_style = if is_selected {
                    self.theme.selected
                } else if agent.is_main {
                    self.theme.title // Main agent gets title styling
                } else {
                    ratatui::style::Style::default()
                };

                // Build multi-span line with right-aligned timestamp
                let label = format!("{}{}", prefix, agent.display_name);
                let timestamp = format!("({})", agent.timestamp_str());

                let label_width = label.width();
                let timestamp_width = timestamp.width();
                let padding_width = available_width
                    .saturating_sub(label_width as u16)
                    .saturating_sub(timestamp_width as u16);

                ListItem::new(Line::from(vec![
                    Span::styled(label, label_style),
                    Span::styled(
                        " ".repeat(padding_width as usize),
                        ratatui::style::Style::default(),
                    ),
                    Span::styled(timestamp, self.theme.timestamp),
                ]))
            })
            .collect();

        let block = Block::default()
            .title(Span::styled(" Agents ", title_style))
            .borders(Borders::ALL)
            .border_style(border_style);

        let list = List::new(items)
            .block(block)
            .highlight_style(self.theme.selected.add_modifier(Modifier::BOLD));

        StatefulWidget::render(list, area, buf, state);
    }
}

pub struct AgentListState {
    pub list_state: ListState,
}

impl AgentListState {
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

impl Default for AgentListState {
    fn default() -> Self {
        Self::new()
    }
}
