use std::collections::VecDeque;

use anyhow::Result;

use crate::logs::{
    Agent, DisplayEntry, ParseResult, Project, Session, SessionWatcher, discover_agents,
    discover_projects, discover_sessions, merge_tool_results, parse_jsonl_file,
};
use crate::ui::{AgentListState, ConversationState, ProjectListState, SessionListState, Theme};

/// Maximum number of conversation entries to keep in memory.
/// When exceeded, oldest entries are dropped.
const MAX_CONVERSATION_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Projects,
    Sessions,
    Agents,
    Conversation,
}

pub struct App {
    pub focus: FocusPane,
    pub projects: Vec<Project>,
    pub sessions: Vec<Session>,
    pub agents: Vec<Agent>,
    pub conversation: VecDeque<DisplayEntry>,
    pub project_state: ProjectListState,
    pub session_state: SessionListState,
    pub agent_state: AgentListState,
    pub conversation_state: ConversationState,
    pub theme: Theme,
    pub watcher: SessionWatcher,
    pub show_thinking: bool,
    pub expand_tools: bool,
    pub show_help: bool,
    pub viewport_height: Option<usize>,
    pub error_message: Option<String>,
    /// Number of entries dropped from the front due to MAX_CONVERSATION_ENTRIES limit
    pub entries_truncated: usize,
    /// Parse errors encountered (line number and error message)
    pub parse_errors: Vec<String>,
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
            agents: Vec::new(),
            conversation: VecDeque::new(),
            project_state: ProjectListState::new(),
            session_state: SessionListState::new(),
            agent_state: AgentListState::new(),
            conversation_state: ConversationState::new(),
            theme,
            watcher: SessionWatcher::new(),
            show_thinking: false,
            expand_tools: true,
            show_help: false,
            viewport_height: None,
            error_message: None,
            entries_truncated: 0,
            parse_errors: Vec::new(),
        };

        // Load initial agents and conversation if there's a session
        app.load_agents_for_selected_session();
        app.load_conversation_for_selected_agent();

        Ok(app)
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Projects => FocusPane::Sessions,
            FocusPane::Sessions => FocusPane::Agents,
            FocusPane::Agents => FocusPane::Conversation,
            FocusPane::Conversation => FocusPane::Projects,
        };
    }

    pub fn cycle_focus_reverse(&mut self) {
        self.focus = match self.focus {
            FocusPane::Projects => FocusPane::Conversation,
            FocusPane::Sessions => FocusPane::Projects,
            FocusPane::Agents => FocusPane::Sessions,
            FocusPane::Conversation => FocusPane::Agents,
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
                    self.load_agents_for_selected_session();
                    self.load_conversation_for_selected_agent();
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load sessions: {}", e));
                    self.sessions.clear();
                    self.agents.clear();
                    self.conversation.clear();
                }
            }
        }
    }

    pub fn load_agents_for_selected_session(&mut self) {
        if let Some(idx) = self.session_state.selected()
            && let Some(session) = self.sessions.get(idx)
        {
            match discover_agents(session) {
                Ok(agents) => {
                    self.agents = agents;
                    self.agent_state = AgentListState::new();
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load agents: {}", e));
                    self.agents.clear();
                }
            }
        } else {
            self.agents.clear();
            self.agent_state = AgentListState::new();
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

        // Also refresh agents for the selected session
        self.refresh_agents();
    }

    /// Refresh agents list for current session, preserving selection if possible
    pub fn refresh_agents(&mut self) {
        let Some(session_idx) = self.session_state.selected() else {
            return;
        };
        let Some(session) = self.sessions.get(session_idx) else {
            return;
        };

        let selected_path = self
            .agent_state
            .selected()
            .and_then(|idx| self.agents.get(idx))
            .map(|a| a.log_path.clone());

        match discover_agents(session) {
            Ok(agents) => {
                self.agents = agents;
                // Restore selection by matching log_path
                if let Some(path) = selected_path
                    && let Some(idx) = self.agents.iter().position(|a| a.log_path == path)
                {
                    self.agent_state.select(Some(idx));
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to refresh agents: {}", e));
            }
        }
    }

    pub fn load_conversation_for_selected_agent(&mut self) {
        self.watcher.stop();
        self.entries_truncated = 0;
        self.parse_errors.clear();

        // Clone the path early to avoid borrow issues
        let log_path = self
            .agent_state
            .selected()
            .and_then(|idx| self.agents.get(idx))
            .map(|agent| agent.log_path.clone());

        if let Some(path) = log_path {
            match parse_jsonl_file(&path) {
                Ok(ParseResult {
                    entries,
                    errors,
                    bytes_read,
                }) => {
                    let merged = merge_tool_results(entries);
                    self.conversation = VecDeque::from(merged);
                    self.parse_errors = errors;
                    self.apply_conversation_limit();
                    self.conversation_state = ConversationState::new();
                    self.error_message = None;

                    // Start watching from where we left off (fixes efficiency bug)
                    if let Err(e) = self.watcher.watch(path) {
                        self.error_message = Some(format!("Failed to watch file: {}", e));
                    } else {
                        self.watcher.set_file_position(bytes_read);
                    }
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load conversation: {}", e));
                    self.conversation.clear();
                }
            }
        } else {
            self.conversation.clear();
        }
    }

    /// Enforce MAX_CONVERSATION_ENTRIES limit by dropping oldest entries
    fn apply_conversation_limit(&mut self) {
        while self.conversation.len() > MAX_CONVERSATION_ENTRIES {
            self.conversation.pop_front();
            self.entries_truncated += 1;
        }
    }

    pub fn refresh_conversation(&mut self) {
        if let Some(path) = self.watcher.current_path().cloned() {
            match crate::logs::parse_jsonl_from_position(&path, self.watcher.file_position()) {
                Ok(ParseResult {
                    entries: new_entries,
                    errors,
                    bytes_read: new_pos,
                }) => {
                    self.watcher.set_file_position(new_pos);
                    self.parse_errors.extend(errors);

                    if !new_entries.is_empty() {
                        // Merge new entries (handles results within the new batch)
                        let merged_new = merge_tool_results(new_entries);

                        // Check if last existing entry is a ToolCall that needs its result
                        // merged from the first new entry
                        if let Some(DisplayEntry::ToolCall { id, result, .. }) =
                            self.conversation.back_mut()
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
                        } else {
                            self.conversation.extend(merged_new);
                        }

                        self.apply_conversation_limit();
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

    pub fn selected_agent_name(&self) -> Option<&str> {
        self.agent_state
            .selected()
            .and_then(|idx| self.agents.get(idx))
            .map(|a| a.display_name.as_str())
    }
}

impl Default for App {
    fn default() -> Self {
        // Infallible - returns empty state on error
        Self::new(Theme::default()).unwrap_or_else(|_| Self {
            focus: FocusPane::Projects,
            projects: Vec::new(),
            sessions: Vec::new(),
            agents: Vec::new(),
            conversation: VecDeque::new(),
            project_state: ProjectListState::new(),
            session_state: SessionListState::new(),
            agent_state: AgentListState::new(),
            conversation_state: ConversationState::new(),
            theme: Theme::default(),
            watcher: SessionWatcher::new(),
            show_thinking: false,
            expand_tools: true,
            show_help: false,
            viewport_height: None,
            error_message: Some("Failed to initialize application".to_string()),
            entries_truncated: 0,
            parse_errors: Vec::new(),
        })
    }
}
