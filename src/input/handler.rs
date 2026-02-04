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

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.conversation_state.scroll_down(1, viewport_height);
            Action::Redraw
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.conversation_state.scroll_up(1);
            Action::Redraw
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.conversation_state
                .scroll_down(viewport_height / 2, viewport_height);
            Action::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.conversation_state.scroll_up(viewport_height / 2);
            Action::Redraw
        }
        KeyCode::PageDown => {
            app.conversation_state
                .scroll_down(viewport_height, viewport_height);
            Action::Redraw
        }
        KeyCode::PageUp => {
            app.conversation_state.scroll_up(viewport_height);
            Action::Redraw
        }
        KeyCode::Char('g') => {
            app.conversation_state.scroll_to_top();
            Action::Redraw
        }
        KeyCode::Char('G') => {
            app.conversation_state.scroll_to_bottom(viewport_height);
            Action::Redraw
        }
        _ => Action::None,
    }
}
