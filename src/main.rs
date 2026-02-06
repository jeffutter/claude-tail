#![allow(dead_code)]

mod app;
mod input;
mod logs;
mod themes;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, StatefulWidget},
};

use app::App;
use input::{Action, handle_key_event};
use ui::{
    AgentList, AppLayout, ConversationView, FocusedPane, LayoutConfig, ProjectList, SessionList,
};

#[derive(Parser)]
#[command(name = "claude-tail")]
#[command(about = "TUI for viewing Claude.ai conversation logs")]
struct Args {
    /// Color theme to use
    #[arg(short, long, default_value = "tokyonight-storm")]
    theme: String,

    /// List available themes and exit
    #[arg(long)]
    list_themes: bool,

    /// Enable automatic switching to project/session/agent with most recent activity
    #[arg(short = 's', long)]
    super_follow: bool,

    /// Enable logging to ~/.claude/logs/claude-tail.log (off by default)
    /// Levels: trace, debug, info, warn, error. Can also set via RUST_LOG env var.
    #[arg(long, value_name = "LEVEL")]
    log_level: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging to ~/.claude/logs/claude-tail.log
    // Only enabled if --log-level is specified or RUST_LOG env var is set
    init_logging(args.log_level.as_deref());

    // Handle --list-themes
    if args.list_themes {
        println!("Available themes:");
        for theme in themes::list_themes() {
            let marker = if theme == "tokyonight-storm" {
                " (default)"
            } else {
                ""
            };
            println!("  {}{}", theme, marker);
        }
        println!("\nCustom themes can be added to: ~/.config/claude-tail/themes/");
        return Ok(());
    }

    // Load theme
    let theme = themes::load_theme(&args.theme)?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(theme, args.super_follow)?;

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
    let mut list_refresh_interval = tokio::time::interval(Duration::from_secs(5));
    // Don't let missed ticks accumulate
    list_refresh_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let loop_start = std::time::Instant::now();

        // Draw UI
        let draw_start = std::time::Instant::now();
        terminal.draw(|frame| draw(frame, app))?;
        let draw_elapsed = draw_start.elapsed();
        if draw_elapsed.as_millis() > 50 {
            tracing::warn!("Slow rendering: {:?}", draw_elapsed);
        }

        // Handle events with timeout for file watching
        // Use biased to prioritize keyboard input over file events
        tokio::select! {
            biased;
            // Poll for keyboard events
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if event::poll(Duration::from_millis(0))?
                    && let Event::Key(key) = event::read()? {
                        match handle_key_event(app, key) {
                            Action::Quit => return Ok(()),
                            Action::Redraw => continue,
                            Action::None => {}
                        }
                    }
            }

            // Watch for file changes
            event = app.watcher.next_event() => {
                if let Some(logs::WatcherEvent::FileModified(_)) = event {
                    app.refresh_conversation();
                }
            }

            // Handle async parse completion
            Some(msg) = app.parse_rx.recv() => {
                match msg {
                    app::ParseMessage::Complete { path, result } => {
                        app.handle_parse_complete(path, result);
                    }
                }
            }

            // Handle async discovery completion
            Some(msg) = app.discovery_rx.recv() => {
                match msg {
                    app::DiscoveryMessage::ProjectsDiscovered(result) => {
                        app.handle_projects_discovered(result);
                        app.auto_switch_to_most_recent();
                    }
                    app::DiscoveryMessage::SessionsDiscovered { project_path, result } => {
                        app.handle_sessions_discovered(project_path, result);
                        app.auto_switch_to_most_recent();
                    }
                }
            }

            // Periodically refresh projects and sessions lists
            _ = list_refresh_interval.tick() => {
                app.refresh_projects();
                app.refresh_sessions();
                // Note: auto_switch will be called when discovery completes
            }
        }

        let loop_elapsed = loop_start.elapsed();
        if loop_elapsed.as_millis() > 200 {
            tracing::warn!("Slow event loop: {:?}", loop_elapsed);
        }
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Determine which pane is focused
    let focused_pane = match app.focus {
        app::FocusPane::Projects => FocusedPane::Projects,
        app::FocusPane::Sessions => FocusedPane::Sessions,
        app::FocusPane::Agents => FocusedPane::Agents,
        app::FocusPane::Conversation => FocusedPane::Conversation,
    };

    // Get cached max content widths
    let max_project_width = app.get_project_width();
    let max_session_width = app.get_session_width();
    let max_agent_width = app.get_agent_width();

    let layout_config = LayoutConfig {
        focused_pane,
        max_project_width,
        max_session_width,
        max_agent_width,
    };

    let layout = AppLayout::new(size, layout_config);

    // Update viewport height for scrolling calculations
    app.viewport_height = Some(layout.conversation.height.saturating_sub(2) as usize);

    // Draw header
    draw_header(frame, layout.header, app);

    // Draw projects pane
    let projects_focused = app.focus == app::FocusPane::Projects;
    let projects_collapsed = app.focus != app::FocusPane::Projects;
    let project_list = ProjectList::new(
        &app.projects,
        projects_focused,
        projects_collapsed,
        &app.theme,
    );
    StatefulWidget::render(
        project_list,
        layout.projects,
        frame.buffer_mut(),
        &mut app.project_state.list_state,
    );

    // Draw sessions pane
    let sessions_focused = app.focus == app::FocusPane::Sessions;
    let sessions_collapsed = app.focus != app::FocusPane::Sessions;
    let session_list = SessionList::new(
        &app.sessions,
        sessions_focused,
        sessions_collapsed,
        &app.theme,
    );
    StatefulWidget::render(
        session_list,
        layout.sessions,
        frame.buffer_mut(),
        &mut app.session_state.list_state,
    );

    // Draw agents pane
    let agents_focused = app.focus == app::FocusPane::Agents;
    let agents_collapsed = app.focus != app::FocusPane::Agents;
    let agent_list = AgentList::new(&app.agents, agents_focused, agents_collapsed, &app.theme);
    StatefulWidget::render(
        agent_list,
        layout.agents,
        frame.buffer_mut(),
        &mut app.agent_state.list_state,
    );

    // Draw conversation pane
    let conversation_focused = app.focus == app::FocusPane::Conversation;
    let conversation_view = ConversationView::new(
        &app.conversation,
        conversation_focused,
        &app.theme,
        app.show_thinking,
        app.expand_tools,
        app.is_parsing,
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
    let project_path = app.selected_project_abbreviated_path().unwrap_or_default();
    let session_name = app.selected_session_name().unwrap_or_default();
    let agent_name = app.selected_agent_name().unwrap_or_default();

    let mut spans = vec![
        Span::styled(" claude-tail ", app.theme.title_focused),
        Span::styled("│ ", app.theme.border),
    ];

    if !project_path.is_empty() {
        spans.push(Span::styled(project_path, app.theme.tool_input));
    }

    if !session_name.is_empty() {
        spans.push(Span::styled(" > ", app.theme.border));
        spans.push(Span::styled(session_name, app.theme.assistant_text));
    }

    // Show agent name only if it's not the main agent
    if !agent_name.is_empty() && agent_name != "Main" {
        spans.push(Span::styled(" > ", app.theme.border));
        spans.push(Span::styled(agent_name, app.theme.tool_name));
    }

    let header = Paragraph::new(Line::from(spans));
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

    // Build warning indicators
    let mut warnings = Vec::new();
    if app.entries_truncated > 0 {
        warnings.push(format!("{}+ entries truncated", app.entries_truncated));
    }
    if !app.parse_errors.is_empty() {
        warnings.push(format!("{} parse errors", app.parse_errors.len()));
    }

    let error_text = app
        .error_message
        .as_ref()
        .map(|e| format!(" Error: {} ", e))
        .unwrap_or_default();

    let warning_text = if warnings.is_empty() {
        String::new()
    } else {
        format!(" [{}] ", warnings.join(", "))
    };

    let mut spans = Vec::new();
    if !error_text.is_empty() {
        spans.push(Span::styled(error_text, app.theme.tool_error));
    }
    if !warning_text.is_empty() {
        spans.push(Span::styled(warning_text, app.theme.thinking));
    }
    spans.push(Span::styled(status_text, app.theme.status_bar));

    let status_bar = Paragraph::new(Line::from(spans));
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

fn init_logging(log_level: Option<&str>) {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    // Determine log level:
    // 1. Check RUST_LOG environment variable first
    // 2. Then use --log-level argument
    // 3. Default to off (no logging)
    let filter = if let Ok(env_filter) = EnvFilter::try_from_default_env() {
        // RUST_LOG is set, use it
        env_filter
    } else if let Some(level) = log_level {
        // --log-level argument provided
        EnvFilter::new(format!("claude_tail={}", level))
    } else {
        // No logging by default
        EnvFilter::new("off")
    };

    // If logging is disabled, just set up a no-op subscriber
    if filter.to_string().contains("off") {
        tracing_subscriber::registry().with(filter).init();
        return;
    }

    // Log to ~/.claude/logs/claude-tail.log
    if let Some(home) = dirs::home_dir() {
        let log_dir = home.join(".claude").join("logs");
        if std::fs::create_dir_all(&log_dir).is_ok() {
            let file_appender = tracing_appender::rolling::never(&log_dir, "claude-tail.log");

            tracing_subscriber::registry()
                .with(fmt::layer().with_writer(file_appender).with_ansi(false))
                .with(filter)
                .init();

            return;
        }
    }

    // Fallback: no logging if we can't create the log directory
    tracing_subscriber::registry()
        .with(EnvFilter::new("off"))
        .init();
}
