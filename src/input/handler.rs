use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FocusPane};

pub enum Action {
    Quit,
    None,
    Redraw,
}

pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    // Global keybindings
    match key.code {
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Action::Quit,
        KeyCode::Tab => {
            app.cycle_focus();
            return Action::Redraw;
        }
        KeyCode::BackTab => {
            app.cycle_focus_reverse();
            return Action::Redraw;
        }
        KeyCode::Char('t') => {
            app.toggle_thinking();
            return Action::Redraw;
        }
        KeyCode::Char('e') => {
            app.toggle_tool_expansion();
            return Action::Redraw;
        }
        KeyCode::Char('f') => {
            app.conversation_state.toggle_follow();
            return Action::Redraw;
        }
        KeyCode::Char('?') => {
            app.show_help = !app.show_help;
            return Action::Redraw;
        }
        KeyCode::Char('r') => {
            app.refresh_projects();
            app.refresh_sessions();
            return Action::Redraw;
        }
        _ => {}
    }

    // Pane-specific keybindings
    match app.focus {
        FocusPane::Projects => handle_projects_input(app, key),
        FocusPane::Sessions => handle_sessions_input(app, key),
        FocusPane::Agents => handle_agents_input(app, key),
        FocusPane::Conversation => handle_conversation_input(app, key),
    }
}

fn handle_projects_input(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.project_state.next(app.projects.len());
            app.load_sessions_for_selected_project();
            Action::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.project_state.previous(app.projects.len());
            app.load_sessions_for_selected_project();
            Action::Redraw
        }
        KeyCode::Char('g') => {
            app.project_state.first();
            app.load_sessions_for_selected_project();
            Action::Redraw
        }
        KeyCode::Char('G') => {
            app.project_state.last(app.projects.len());
            app.load_sessions_for_selected_project();
            Action::Redraw
        }
        KeyCode::Enter => {
            app.focus = FocusPane::Sessions;
            Action::Redraw
        }
        _ => Action::None,
    }
}

fn handle_sessions_input(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.session_state.next(app.sessions.len());
            app.load_agents_for_selected_session();
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.session_state.previous(app.sessions.len());
            app.load_agents_for_selected_session();
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Char('g') => {
            app.session_state.first();
            app.load_agents_for_selected_session();
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Char('G') => {
            app.session_state.last(app.sessions.len());
            app.load_agents_for_selected_session();
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Enter => {
            app.focus = FocusPane::Agents;
            Action::Redraw
        }
        _ => Action::None,
    }
}

fn handle_agents_input(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.agent_state.next(app.agents.len());
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.agent_state.previous(app.agents.len());
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Char('g') => {
            app.agent_state.first();
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Char('G') => {
            app.agent_state.last(app.agents.len());
            app.load_conversation_for_selected_agent();
            Action::Redraw
        }
        KeyCode::Enter => {
            app.focus = FocusPane::Conversation;
            Action::Redraw
        }
        _ => Action::None,
    }
}

fn handle_conversation_input(app: &mut App, key: KeyEvent) -> Action {
    let viewport_height = app.viewport_height.unwrap_or(20);
    let threshold = viewport_height / 2; // Trigger load when within half viewport of edge

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.conversation_state.scroll_down(1, viewport_height);
            check_and_trigger_load(app, threshold, 20);
            Action::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.conversation_state.scroll_up(1);
            check_and_trigger_load(app, threshold, 20);
            Action::Redraw
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.conversation_state
                .scroll_down(viewport_height / 2, viewport_height);
            check_and_trigger_load(app, threshold, 30);
            Action::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.conversation_state.scroll_up(viewport_height / 2);
            check_and_trigger_load(app, threshold, 30);
            Action::Redraw
        }
        KeyCode::PageDown => {
            app.conversation_state
                .scroll_down(viewport_height, viewport_height);
            check_and_trigger_load(app, threshold, 40);
            Action::Redraw
        }
        KeyCode::PageUp => {
            app.conversation_state.scroll_up(viewport_height);
            check_and_trigger_load(app, threshold, 40);
            Action::Redraw
        }
        KeyCode::Char('g') => {
            app.conversation_state.scroll_to_top();
            // Request jump to start
            if let Some((path, start, end)) = app.buffer.request_jump_to_start() {
                spawn_scroll_load(app, path, start, end);
            }
            app.conversation_state.follow_mode = false;
            Action::Redraw
        }
        KeyCode::Char('G') => {
            app.conversation_state.scroll_to_bottom(viewport_height);
            // Request jump to end
            if let Some((path, start, end)) = app.buffer.request_jump_to_end() {
                spawn_scroll_load(app, path, start, end);
            }
            app.conversation_state.follow_mode = true;
            Action::Redraw
        }
        _ => Action::None,
    }
}

/// Check if we're near buffer edges and trigger async load if needed
fn check_and_trigger_load(app: &mut App, threshold: usize, load_count: usize) {
    let scroll_offset = app.conversation_state.scroll_offset;
    let total_lines = app.conversation_state.total_lines;

    // Near top - load older entries
    if scroll_offset < threshold && app.buffer.has_older()
        && let Some((path, start, end)) = app.buffer.request_load_older(load_count) {
            spawn_scroll_load(app, path, start, end);
        }

    // Near bottom - load newer entries
    if scroll_offset > total_lines.saturating_sub(threshold) && app.buffer.has_newer()
        && let Some((path, start, end)) = app.buffer.request_load_newer(load_count) {
            spawn_scroll_load(app, path, start, end);
        }
}

/// Spawn an async task to parse a byte range
fn spawn_scroll_load(app: &mut App, path: std::path::PathBuf, start: u64, end: u64) {
    use crate::app::ParseMessage;
    use crate::logs::parse_jsonl_range_async;

    let tx = app.parse_tx.clone();
    tokio::spawn(async move {
        let result = parse_jsonl_range_async(path.clone(), start, end).await;
        let _ = tx.send(ParseMessage::Complete { path, result });
    });
}
