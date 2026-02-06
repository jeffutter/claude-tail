use std::collections::VecDeque;
use std::path::PathBuf;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::logs::{
    Agent, DisplayEntry, ParseResult, Project, Session, SessionWatcher, discover_agents,
    discover_projects, discover_sessions, merge_tool_results, parse_jsonl_file_async,
    parse_jsonl_from_position_async,
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

/// Message sent when async parsing completes
pub enum ParseMessage {
    Complete {
        path: PathBuf,
        result: Result<ParseResult>,
    },
}

/// Message sent when async project/session discovery completes
pub enum DiscoveryMessage {
    ProjectsDiscovered(Result<Vec<Project>>),
    SessionsDiscovered {
        project_path: PathBuf,
        result: Result<Vec<Session>>,
    },
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
    /// Channel receiver for async parse results
    pub parse_rx: mpsc::UnboundedReceiver<ParseMessage>,
    /// Channel sender for async parse results
    parse_tx: mpsc::UnboundedSender<ParseMessage>,
    /// Whether a parse operation is currently in progress
    pub is_parsing: bool,
    /// Path of the file currently being parsed
    pub parsing_path: Option<PathBuf>,
    /// Whether a refresh operation is currently in progress
    pub is_refreshing: bool,
    /// Channel receiver for async discovery results
    pub discovery_rx: mpsc::UnboundedReceiver<DiscoveryMessage>,
    /// Channel sender for async discovery results
    discovery_tx: mpsc::UnboundedSender<DiscoveryMessage>,
    /// Whether to automatically switch to most recent project/session/agent
    pub super_follow_enabled: bool,
}

impl App {
    pub fn new(theme: Theme, super_follow_enabled: bool) -> Result<Self> {
        let projects = discover_projects().unwrap_or_default();
        let sessions = if !projects.is_empty() {
            discover_sessions(&projects[0]).unwrap_or_default()
        } else {
            Vec::new()
        };

        let (parse_tx, parse_rx) = mpsc::unbounded_channel();
        let (discovery_tx, discovery_rx) = mpsc::unbounded_channel();

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
            parse_rx,
            parse_tx,
            is_parsing: false,
            parsing_path: None,
            is_refreshing: false,
            discovery_rx,
            discovery_tx,
            super_follow_enabled,
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

    /// Refresh projects list by spawning a background discovery task
    pub fn refresh_projects(&mut self) {
        let tx = self.discovery_tx.clone();
        tokio::spawn(async move {
            let result = discover_projects();
            let _ = tx.send(DiscoveryMessage::ProjectsDiscovered(result));
        });
    }

    /// Handle completion of background project discovery
    pub fn handle_projects_discovered(&mut self, result: Result<Vec<Project>>) {
        let selected_path = self
            .project_state
            .selected()
            .and_then(|idx| self.projects.get(idx))
            .map(|p| p.path.clone());

        match result {
            Ok(projects) => {
                let was_empty = self.projects.is_empty();
                self.projects = projects;
                // Restore selection by matching path, or select first if we had no valid selection
                if let Some(path) = selected_path
                    && let Some(idx) = self.projects.iter().position(|p| p.path == path)
                {
                    self.project_state.select(Some(idx));
                } else if !self.projects.is_empty() {
                    // No previous selection or it's gone - select first item
                    self.project_state.select(Some(0));
                    // If we just got projects after being empty, load sessions too
                    if was_empty {
                        self.load_sessions_for_selected_project();
                    }
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to refresh projects: {}", e));
            }
        }
    }

    /// Refresh sessions list by spawning a background discovery task
    pub fn refresh_sessions(&mut self) {
        let Some(project_idx) = self.project_state.selected() else {
            return;
        };
        let Some(project) = self.projects.get(project_idx).cloned() else {
            return;
        };

        let tx = self.discovery_tx.clone();
        let project_path = project.path.clone();
        tokio::spawn(async move {
            let result = discover_sessions(&project);
            let _ = tx.send(DiscoveryMessage::SessionsDiscovered {
                project_path,
                result,
            });
        });
    }

    /// Handle completion of background session discovery
    pub fn handle_sessions_discovered(
        &mut self,
        project_path: PathBuf,
        result: Result<Vec<Session>>,
    ) {
        // Only apply if this is still for the currently selected project
        let current_project_path = self
            .project_state
            .selected()
            .and_then(|idx| self.projects.get(idx))
            .map(|p| &p.path);

        if current_project_path != Some(&project_path) {
            // Discovery result is stale (user changed projects), ignore it
            return;
        }

        let selected_path = self
            .session_state
            .selected()
            .and_then(|idx| self.sessions.get(idx))
            .map(|s| s.log_path.clone());

        match result {
            Ok(sessions) => {
                let was_empty = self.sessions.is_empty();
                self.sessions = sessions;
                // Restore selection by matching log_path, or select first if no valid selection
                if let Some(path) = selected_path
                    && let Some(idx) = self.sessions.iter().position(|s| s.log_path == path)
                {
                    self.session_state.select(Some(idx));
                } else if !self.sessions.is_empty() {
                    // No previous selection or it's gone - select first item
                    self.session_state.select(Some(0));
                    // If we just got sessions after being empty, load agents too
                    if was_empty {
                        self.load_agents_for_selected_session();
                    }
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
                let was_empty = self.agents.is_empty();
                self.agents = agents;
                // Restore selection by matching log_path, or select first if no valid selection
                if let Some(path) = selected_path
                    && let Some(idx) = self.agents.iter().position(|a| a.log_path == path)
                {
                    self.agent_state.select(Some(idx));
                } else if !self.agents.is_empty() {
                    // No previous selection or it's gone - select first item
                    self.agent_state.select(Some(0));
                    // If we just got agents after being empty, load conversation too
                    if was_empty {
                        self.load_conversation_for_selected_agent();
                    }
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to refresh agents: {}", e));
            }
        }
    }

    pub fn load_conversation_for_selected_agent(&mut self) {
        // Prevent duplicate parses
        if self.is_parsing {
            return;
        }

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
            // Mark as loading and clear previous conversation
            self.is_parsing = true;
            self.parsing_path = Some(path.clone());
            self.conversation.clear();
            self.error_message = None;

            // Spawn async parsing task
            let tx = self.parse_tx.clone();
            tokio::spawn(async move {
                let result = parse_jsonl_file_async(path.clone()).await;
                let _ = tx.send(ParseMessage::Complete { path, result });
            });
        } else {
            self.conversation.clear();
            self.is_parsing = false;
            self.parsing_path = None;
        }
    }

    /// Handle completed parse result from async task
    pub fn handle_parse_complete(&mut self, path: PathBuf, result: Result<ParseResult>) {
        // Determine if this is an initial parse or incremental refresh
        // Invariant: Only one of these can be true at a time
        //   - is_parsing: true during initial load (parsing_path == path)
        //   - is_refreshing: true during incremental refresh (watcher path == path)
        let is_initial = self.is_parsing && self.parsing_path.as_ref() == Some(&path);
        let is_refresh = self.is_refreshing && self.watcher.current_path() == Some(&path);

        // Only process if this is a valid parse request (either initial or refresh)
        if !is_initial && !is_refresh {
            return;
        }

        // Clear the appropriate state flag
        if is_initial {
            self.is_parsing = false;
            self.parsing_path = None;
        }
        if is_refresh {
            self.is_refreshing = false;
        }

        match result {
            Ok(ParseResult {
                entries,
                errors,
                bytes_read,
            }) => {
                if is_initial {
                    // Initial load: replace conversation entirely
                    let merged = merge_tool_results(entries);
                    self.conversation = VecDeque::from(merged);
                    self.parse_errors = errors;
                    self.apply_conversation_limit();
                    self.conversation_state = ConversationState::new();
                    self.error_message = None;

                    // Start watching from where we left off
                    if let Err(e) = self.watcher.watch(path) {
                        self.error_message = Some(format!("Failed to watch file: {}", e));
                    } else {
                        self.watcher.set_file_position(bytes_read);
                    }
                } else if is_refresh {
                    // Incremental refresh: append new entries
                    self.watcher.set_file_position(bytes_read);
                    self.parse_errors.extend(errors);

                    if !entries.is_empty() {
                        // Merge new entries (handles results within the new batch)
                        let merged_new = merge_tool_results(entries);

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
            }
            Err(e) => {
                if is_initial {
                    self.error_message = Some(format!("Failed to load conversation: {}", e));
                    self.conversation.clear();
                } else if is_refresh {
                    self.error_message = Some(format!("Failed to refresh: {}", e));
                }
            }
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
        // Prevent duplicate refreshes
        if self.is_refreshing {
            return;
        }

        if let Some(path) = self.watcher.current_path().cloned() {
            let position = self.watcher.file_position();
            self.is_refreshing = true;

            // Spawn async parsing task
            let tx = self.parse_tx.clone();
            tokio::spawn(async move {
                let result = parse_jsonl_from_position_async(path.clone(), position).await;
                let _ = tx.send(ParseMessage::Complete { path, result });
            });
        }
    }

    pub fn selected_project_name(&self) -> Option<&str> {
        self.project_state
            .selected()
            .and_then(|idx| self.projects.get(idx))
            .map(|p| p.name.as_str())
    }

    /// Returns the abbreviated path of the selected project (compressed intermediate dirs)
    pub fn selected_project_abbreviated_path(&self) -> Option<String> {
        self.project_state
            .selected()
            .and_then(|idx| self.projects.get(idx))
            .map(|p| p.abbreviated_path())
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

    /// Automatically switch to the project/session/agent with most recent activity
    /// Only operates if super_follow_enabled is true
    pub fn auto_switch_to_most_recent(&mut self) {
        if !self.super_follow_enabled {
            return;
        }

        // Projects are already sorted by last_modified descending, so most recent is at index 0
        if !self.projects.is_empty() && self.project_state.selected() != Some(0) {
            self.project_state.select(Some(0));
            self.load_sessions_for_selected_project();
        }

        // Sessions are already sorted by last_modified descending, so most recent is at index 0
        if !self.sessions.is_empty() && self.session_state.selected() != Some(0) {
            self.session_state.select(Some(0));
            self.load_agents_for_selected_session();
        }

        // For agents, prefer the first sub-agent (index 1) if it exists, otherwise main agent (index 0)
        // Agents list has main agent at 0, followed by sub-agents sorted by last_modified
        if !self.agents.is_empty() {
            let target_idx = if self.agents.len() > 1 { 1 } else { 0 };
            if self.agent_state.selected() != Some(target_idx) {
                self.agent_state.select(Some(target_idx));
                self.load_conversation_for_selected_agent();
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        // Infallible - returns empty state on error
        Self::new(Theme::default(), false).unwrap_or_else(|_| {
            let (parse_tx, parse_rx) = mpsc::unbounded_channel();
            let (discovery_tx, discovery_rx) = mpsc::unbounded_channel();
            Self {
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
                parse_rx,
                parse_tx,
                is_parsing: false,
                parsing_path: None,
                is_refreshing: false,
                discovery_rx,
                discovery_tx,
                super_follow_enabled: false,
            }
        })
    }
}
