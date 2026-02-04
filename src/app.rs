use anyhow::Result;

use crate::logs::{
    DisplayEntry, Project, Session, SessionWatcher, discover_projects, discover_sessions,
    merge_tool_results, parse_jsonl_file,
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
    pub fn new(theme: Theme) -> Result<Self> {
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
            theme,
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
        if let Some(idx) = self.project_state.selected()
            && let Some(project) = self.projects.get(idx)
        {
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

    /// Refresh projects list, preserving selection if possible
    pub fn refresh_projects(&mut self) {
        let selected_path = self
            .project_state
            .selected()
            .and_then(|idx| self.projects.get(idx))
            .map(|p| p.path.clone());

        match discover_projects() {
            Ok(projects) => {
                self.projects = projects;
                // Restore selection by matching path
                if let Some(path) = selected_path
                    && let Some(idx) = self.projects.iter().position(|p| p.path == path)
                {
                    self.project_state.select(Some(idx));
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to refresh projects: {}", e));
            }
        }
    }

    /// Refresh sessions list for current project, preserving selection if possible
    pub fn refresh_sessions(&mut self) {
        let Some(project_idx) = self.project_state.selected() else {
            return;
        };
        let Some(project) = self.projects.get(project_idx) else {
            return;
        };

        let selected_path = self
            .session_state
            .selected()
            .and_then(|idx| self.sessions.get(idx))
            .map(|s| s.log_path.clone());

        match discover_sessions(project) {
            Ok(sessions) => {
                self.sessions = sessions;
                // Restore selection by matching log_path
                if let Some(path) = selected_path
                    && let Some(idx) = self.sessions.iter().position(|s| s.log_path == path)
                {
                    self.session_state.select(Some(idx));
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to refresh sessions: {}", e));
            }
        }
    }

    pub fn load_conversation_for_selected_session(&mut self) {
        self.watcher.stop();

        if let Some(idx) = self.session_state.selected() {
            if let Some(session) = self.sessions.get(idx) {
                match parse_jsonl_file(&session.log_path) {
                    Ok(entries) => {
                        self.conversation = merge_tool_results(entries);
                        self.conversation_state = ConversationState::new();
                        self.error_message = None;

                        // Start watching this file
                        if let Err(e) = self.watcher.watch(session.log_path.clone()) {
                            self.error_message = Some(format!("Failed to watch file: {}", e));
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
                        // Merge new entries (handles results within the new batch)
                        let merged_new = merge_tool_results(new_entries);

                        // Check if last existing entry is a ToolCall that needs its result
                        // merged from the first new entry
                        if let Some(DisplayEntry::ToolCall { id, result, .. }) =
                            self.conversation.last_mut()
                            && result.is_none()
                            && let Some(DisplayEntry::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                                ..
                            }) = merged_new.first()
                            && tool_use_id == id
                        {
                            // Merge the result into the existing ToolCall
                            *result = Some(crate::logs::ToolCallResult {
                                content: content.clone(),
                                is_error: *is_error,
                            });
                            // Skip the first entry since we merged it
                            self.conversation.extend(merged_new.into_iter().skip(1));
                            return;
                        }

                        self.conversation.extend(merged_new);
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

    /// Returns the full path of the selected project, relative to home (~)
    pub fn selected_project_path(&self) -> Option<String> {
        self.project_state
            .selected()
            .and_then(|idx| self.projects.get(idx))
            .map(|p| {
                let path = p.original_path.to_string_lossy();
                let home = std::env::var("HOME").unwrap_or_default();
                if !home.is_empty() && path.starts_with(&home) {
                    format!("~{}", &path[home.len()..])
                } else {
                    path.to_string()
                }
            })
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
        Self::new(Theme::default()).expect("Failed to create App")
    }
}
