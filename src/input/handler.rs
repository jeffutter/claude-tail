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
            use crate::logs::parse_jsonl_range;

            // Jump to start - synchronous load
            if let Some((path, start, end)) = app.buffer.request_jump_to_start() {
                let content_width = app.viewport_width.unwrap_or(80);
                let result = parse_jsonl_range(&path, start, end);
                app.buffer.receive_loaded(
                    result,
                    content_width,
                    app.show_thinking,
                    app.expand_tools,
                );
            }
            app.conversation_state.scroll_to_top();
            app.conversation_state.follow_mode = false;
            Action::Redraw
        }
        KeyCode::Char('G') => {
            use crate::logs::parse_jsonl_range;

            // Jump to end - synchronous load
            if let Some((path, start, end)) = app.buffer.request_jump_to_end() {
                let content_width = app.viewport_width.unwrap_or(80);
                let result = parse_jsonl_range(&path, start, end);
                app.buffer.receive_loaded(
                    result,
                    content_width,
                    app.show_thinking,
                    app.expand_tools,
                );
            }
            app.conversation_state.scroll_to_bottom(viewport_height);
            app.conversation_state.follow_mode = true;
            Action::Redraw
        }
        _ => Action::None,
    }
}

/// Check if we're near buffer edges and trigger loads.
/// When loading is needed, loads multiple batches to fill the buffer toward the edge,
/// rather than stopping after one batch when scroll_delta pushes offset above threshold.
fn check_and_trigger_load(app: &mut App, threshold: usize, load_count: usize) {
    use crate::logs::parse_jsonl_range;

    let content_width = app.viewport_width.unwrap_or(80);
    let viewport_height = app.viewport_height.unwrap_or(20);
    let scroll_offset = app.conversation_state.scroll_offset;

    // Near top - load older entries
    if scroll_offset < threshold && app.buffer.has_older() {
        // Load up to 5 batches to fill the buffer toward the beginning
        for _ in 0..5 {
            if !app.buffer.has_older() {
                break;
            }
            app.buffer.clear_rate_limit();
            if let Some((path, start, end)) = app.buffer.request_load_older(load_count) {
                let result = parse_jsonl_range(&path, start, end);
                let scroll_delta = app.buffer.receive_loaded(
                    result,
                    content_width,
                    app.show_thinking,
                    app.expand_tools,
                );
                if scroll_delta != 0 {
                    app.conversation_state.scroll_offset =
                        (app.conversation_state.scroll_offset as isize + scroll_delta).max(0)
                            as usize;
                }
                tracing::debug!(
                    scroll_delta,
                    new_offset = app.conversation_state.scroll_offset,
                    win_start = app.buffer.window_position().0,
                    win_end = app.buffer.window_position().1,
                    "Loaded older batch"
                );
            } else {
                break;
            }
        }
    }

    // Near bottom - load newer entries
    let scroll_offset = app.conversation_state.scroll_offset;
    if scroll_offset
        > app
            .conversation_state
            .total_lines
            .saturating_sub(viewport_height + threshold)
        && app.buffer.has_newer()
    {
        for _ in 0..5 {
            if !app.buffer.has_newer() {
                break;
            }
            app.buffer.clear_rate_limit();
            if let Some((path, start, end)) = app.buffer.request_load_newer(load_count) {
                let result = parse_jsonl_range(&path, start, end);
                let scroll_delta = app.buffer.receive_loaded(
                    result,
                    content_width,
                    app.show_thinking,
                    app.expand_tools,
                );
                if scroll_delta != 0 {
                    app.conversation_state.scroll_offset =
                        (app.conversation_state.scroll_offset as isize + scroll_delta).max(0)
                            as usize;
                }
            } else {
                break;
            }
        }
    }

    // Update total_lines after loading
    app.conversation_state.total_lines =
        app.buffer
            .total_rendered_lines(content_width, app.show_thinking, app.expand_tools);
}
