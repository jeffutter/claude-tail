use anyhow::Result;

use crate::logs::{
    discover_projects, discover_sessions, parse_jsonl_file, DisplayEntry, Project, Session,
    SessionWatcher,
};
use crate::ui::{ConversationState, ProjectListState, SessionListState, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Projects,
    Sessions,
    Conversation,
}

pub struct App {
    pub focus: FocusPane,
    pub projects: Vec<Project>,
    pub sessions: Vec<Session>,
    pub conversation: Vec<DisplayEntry>,
    pub project_state: ProjectListState,
    pub session_state: SessionListState,
    pub conversation_state: ConversationState,
    pub theme: Theme,
    pub watcher: SessionWatcher,
    pub show_thinking: bool,
    pub expand_tools: bool,
    pub show_help: bool,
    pub viewport_height: Option<usize>,
    pub error_message: Option<String>,
}

impl App {
    pub fn new() -> Result<Self> {
        let projects = discover_projects().unwrap_or_default();
        let sessions = if !projects.is_empty() {
            discover_sessions(&projects[0]).unwrap_or_default()
        } else {
            Vec::new()
        };

        let mut app = Self {
            focus: FocusPane::Projects,
            projects,
            sessions,
            conversation: Vec::new(),
            project_state: ProjectListState::new(),
            session_state: SessionListState::new(),
            conversation_state: ConversationState::new(),
            theme: Theme::new(),
            watcher: SessionWatcher::new(),
            show_thinking: false,
            expand_tools: true,
            show_help: false,
            viewport_height: None,
            error_message: None,
        };

        // Load initial conversation if there's a session
        app.load_conversation_for_selected_session();

        Ok(app)
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Projects => FocusPane::Sessions,
            FocusPane::Sessions => FocusPane::Conversation,
            FocusPane::Conversation => FocusPane::Projects,
        };
    }

    pub fn cycle_focus_reverse(&mut self) {
        self.focus = match self.focus {
            FocusPane::Projects => FocusPane::Conversation,
            FocusPane::Sessions => FocusPane::Projects,
            FocusPane::Conversation => FocusPane::Sessions,
        };
    }

    pub fn toggle_thinking(&mut self) {
        self.show_thinking = !self.show_thinking;
    }

    pub fn toggle_tool_expansion(&mut self) {
        self.expand_tools = !self.expand_tools;
    }

    pub fn load_sessions_for_selected_project(&mut self) {
        if let Some(idx) = self.project_state.selected() {
            if let Some(project) = self.projects.get(idx) {
                match discover_sessions(project) {
                    Ok(sessions) => {
                        self.sessions = sessions;
                        self.session_state = SessionListState::new();
                        self.load_conversation_for_selected_session();
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to load sessions: {}", e));
                        self.sessions.clear();
                        self.conversation.clear();
                    }
                }
            }
        }
    }

    pub fn load_conversation_for_selected_session(&mut self) {
        self.watcher.stop();

        if let Some(idx) = self.session_state.selected() {
            if let Some(session) = self.sessions.get(idx) {
                match parse_jsonl_file(&session.log_path) {
                    Ok(entries) => {
                        self.conversation = entries;
                        self.conversation_state = ConversationState::new();
                        self.error_message = None;

                        // Start watching this file
                        if let Err(e) = self.watcher.watch(session.log_path.clone()) {
                            self.error_message =
                                Some(format!("Failed to watch file: {}", e));
                        }
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to load conversation: {}", e));
                        self.conversation.clear();
                    }
                }
            }
        } else {
            self.conversation.clear();
        }
    }

    pub fn refresh_conversation(&mut self) {
        if let Some(path) = self.watcher.current_path().cloned() {
            match crate::logs::parse_jsonl_from_position(&path, self.watcher.file_position()) {
                Ok((new_entries, new_pos)) => {
                    self.watcher.set_file_position(new_pos);
                    if !new_entries.is_empty() {
                        self.conversation.extend(new_entries);
                    }
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to refresh: {}", e));
                }
            }
        }
    }

    pub fn selected_project_name(&self) -> Option<&str> {
        self.project_state
            .selected()
            .and_then(|idx| self.projects.get(idx))
            .map(|p| p.name.as_str())
    }

    pub fn selected_session_name(&self) -> Option<String> {
        self.session_state
            .selected()
            .and_then(|idx| self.sessions.get(idx))
            .map(|s| s.display_name())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new().expect("Failed to create App")
    }
}
