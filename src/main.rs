#![allow(dead_code)]

mod app;
mod input;
mod logs;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, StatefulWidget},
    Frame, Terminal,
};

use app::App;
use input::{handle_key_event, Action};
use ui::{AppLayout, ConversationView, ProjectList, SessionList};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new()?;

    // Run main loop
    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()>
where
    B::Error: Send + Sync + 'static,
{
    loop {
        // Draw UI
        terminal.draw(|frame| draw(frame, app))?;

        // Handle events with timeout for file watching
        tokio::select! {
            // Poll for keyboard events
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if event::poll(Duration::from_millis(0))? {
                    if let Event::Key(key) = event::read()? {
                        match handle_key_event(app, key) {
                            Action::Quit => return Ok(()),
                            Action::Redraw => continue,
                            Action::None => {}
                        }
                    }
                }
            }

            // Watch for file changes
            event = app.watcher.next_event() => {
                if let Some(logs::WatcherEvent::FileModified(_)) = event {
                    app.refresh_conversation();
                }
            }
        }
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();
    let layout = AppLayout::new(size);

    // Update viewport height for scrolling calculations
    app.viewport_height = Some(layout.conversation.height.saturating_sub(2) as usize);

    // Draw header
    draw_header(frame, layout.header, app);

    // Draw projects pane
    let projects_focused = app.focus == app::FocusPane::Projects;
    let project_list = ProjectList::new(&app.projects, projects_focused, &app.theme);
    StatefulWidget::render(
        project_list,
        layout.projects,
        frame.buffer_mut(),
        &mut app.project_state.list_state,
    );

    // Draw sessions pane
    let sessions_focused = app.focus == app::FocusPane::Sessions;
    let session_list = SessionList::new(&app.sessions, sessions_focused, &app.theme);
    StatefulWidget::render(
        session_list,
        layout.sessions,
        frame.buffer_mut(),
        &mut app.session_state.list_state,
    );

    // Draw conversation pane
    let conversation_focused = app.focus == app::FocusPane::Conversation;
    let conversation_view = ConversationView::new(
        &app.conversation,
        conversation_focused,
        &app.theme,
        app.show_thinking,
        app.expand_tools,
    );
    StatefulWidget::render(
        conversation_view,
        layout.conversation,
        frame.buffer_mut(),
        &mut app.conversation_state,
    );

    // Draw status bar
    draw_status_bar(frame, layout.status_bar, app);

    // Draw help overlay if enabled
    if app.show_help {
        draw_help_overlay(frame, size);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = format!(
        " claude-tail | {} > {} ",
        app.selected_project_name().unwrap_or(""),
        app.selected_session_name().unwrap_or_default()
    );

    let header = Paragraph::new(Line::from(vec![
        Span::styled(title, app.theme.title_focused),
    ]));

    frame.render_widget(header, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let follow_indicator = if app.conversation_state.follow_mode {
        "[F]ollow ON"
    } else {
        "[f]ollow off"
    };

    let thinking_indicator = if app.show_thinking {
        "[T]hinking ON"
    } else {
        "[t]hinking off"
    };

    let expand_indicator = if app.expand_tools {
        "[E]xpand ON"
    } else {
        "[e]xpand off"
    };

    let status_text = format!(
        " [q]uit [Tab] pane [j/k] nav [g/G] top/bottom  {}  {}  {}  [?] help ",
        follow_indicator, thinking_indicator, expand_indicator
    );

    let error_text = app
        .error_message
        .as_ref()
        .map(|e| format!(" Error: {} ", e))
        .unwrap_or_default();

    let line = if error_text.is_empty() {
        Line::from(Span::styled(status_text, app.theme.status_bar))
    } else {
        Line::from(vec![
            Span::styled(error_text, app.theme.tool_error),
            Span::styled(status_text, app.theme.status_bar),
        ])
    };

    let status_bar = Paragraph::new(line);
    frame.render_widget(status_bar, area);
}

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let help_width = 50;
    let help_height = 18;
    let x = (area.width.saturating_sub(help_width)) / 2;
    let y = (area.height.saturating_sub(help_height)) / 2;

    let help_area = Rect::new(x, y, help_width, help_height);

    // Clear the area
    frame.render_widget(Clear, help_area);

    let help_text = vec![
        Line::from(""),
        Line::from("  Navigation"),
        Line::from("  ──────────"),
        Line::from("  Tab / Shift+Tab   Cycle panes"),
        Line::from("  j / Down          Move down / scroll"),
        Line::from("  k / Up            Move up / scroll"),
        Line::from("  g                 Go to top"),
        Line::from("  G                 Go to bottom"),
        Line::from("  Enter             Select / enter pane"),
        Line::from(""),
        Line::from("  Display"),
        Line::from("  ───────"),
        Line::from("  t                 Toggle thinking blocks"),
        Line::from("  e                 Toggle tool expansion"),
        Line::from("  f                 Toggle follow mode"),
        Line::from(""),
        Line::from("  q / Ctrl+C        Quit"),
    ];

    let help = Paragraph::new(help_text).block(
        Block::default()
            .title(" Help ")
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(ratatui::style::Color::Cyan)),
    );

    frame.render_widget(help, help_area);
}
