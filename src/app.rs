use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc;

use crate::logs::{
    Agent, EntryBuffer, ParseResult, Project, Session, SessionWatcher, discover_agents,
    discover_projects, discover_sessions,
};
use crate::ui::{AgentListState, ConversationState, ProjectListState, SessionListState, Theme};

/// Maximum number of JSONL lines to keep in buffer.
/// When exceeded during scrolling, oldest/newest entries are evicted.
const BUFFER_CAPACITY: usize = 100;

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
    pub buffer: EntryBuffer,
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
    /// Channel receiver for async parse results (now used for scroll loads)
    pub parse_rx: mpsc::UnboundedReceiver<ParseMessage>,
    /// Channel sender for async parse results
    pub parse_tx: mpsc::UnboundedSender<ParseMessage>,
    /// Channel receiver for async discovery results
    pub discovery_rx: mpsc::UnboundedReceiver<DiscoveryMessage>,
    /// Channel sender for async discovery results
    discovery_tx: mpsc::UnboundedSender<DiscoveryMessage>,
    /// Whether to automatically switch to most recent project/session/agent
    pub super_follow_enabled: bool,
    /// Cached maximum content width for projects list
    cached_project_width: Option<u16>,
    /// Cached maximum content width for sessions list
    cached_session_width: Option<u16>,
    /// Cached maximum content width for agents list
    cached_agent_width: Option<u16>,
    /// Last time we refreshed the conversation from file watcher
    last_conversation_refresh: Option<Instant>,
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
            buffer: EntryBuffer::new(BUFFER_CAPACITY),
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
            parse_rx,
            parse_tx,
            discovery_rx,
            discovery_tx,
            super_follow_enabled,
            cached_project_width: None,
            cached_session_width: None,
            cached_agent_width: None,
            last_conversation_refresh: None,
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
                    self.cached_session_width = None; // Invalidate cache
                    self.session_state = SessionListState::new();
                    self.load_agents_for_selected_session();
                    self.load_conversation_for_selected_agent();
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load sessions: {}", e));
                    self.sessions.clear();
                    self.cached_session_width = None; // Invalidate cache
                    self.agents.clear();
                    self.cached_agent_width = None; // Invalidate cache
                    // Buffer will be cleared on next agent load
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
                    self.cached_agent_width = None; // Invalidate cache
                    self.agent_state = AgentListState::new();
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load agents: {}", e));
                    self.agents.clear();
                    self.cached_agent_width = None; // Invalidate cache
                }
            }
        } else {
            self.agents.clear();
            self.cached_agent_width = None; // Invalidate cache
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
                self.cached_project_width = None; // Invalidate cache
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
                self.cached_session_width = None; // Invalidate cache
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
                self.cached_agent_width = None; // Invalidate cache
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
        self.watcher.stop();

        // Clone the path early to avoid borrow issues
        let log_path = self
            .agent_state
            .selected()
            .and_then(|idx| self.agents.get(idx))
            .map(|agent| agent.log_path.clone());

        if let Some(path) = log_path {
            self.error_message = None;

            // Synchronously load file with buffer
            match self.buffer.load_file(&path) {
                Ok(()) => {
                    // Start watching from where buffer left off
                    if let Err(e) = self.watcher.watch(path) {
                        self.error_message = Some(format!("Failed to watch file: {}", e));
                    }
                    // Reset conversation state for new file
                    self.conversation_state = ConversationState::new();
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to load conversation: {}", e));
                }
            }
        }
    }

    /// Handle completed parse result from async scroll load
    pub fn handle_parse_complete(&mut self, _path: PathBuf, result: Result<ParseResult>) {
        // Get content width for line calculations
        let content_width = self.viewport_height.unwrap_or(80);

        let (added, evicted) = self.buffer.receive_loaded(
            result,
            content_width,
            self.show_thinking,
            self.expand_tools,
        );

        // Adjust scroll_offset based on what was added/evicted
        if added > 0 {
            // Content was prepended (Older) - shift scroll down
            self.conversation_state.scroll_offset += added;
        }
        if evicted > 0 {
            // Content was evicted from front - shift scroll up
            self.conversation_state.scroll_offset = self
                .conversation_state
                .scroll_offset
                .saturating_sub(evicted);
        }
    }

    pub fn refresh_conversation(&mut self) {
        // Rate limit: Don't refresh more than once per second
        // This prevents flooding from actively-written files
        if let Some(last_refresh) = self.last_conversation_refresh
            && last_refresh.elapsed() < Duration::from_secs(1)
        {
            return;
        }

        if self.watcher.current_path().is_some() {
            self.last_conversation_refresh = Some(Instant::now());

            // Synchronous tail update
            if let Err(e) = self
                .buffer
                .file_changed(self.conversation_state.follow_mode)
            {
                self.error_message = Some(format!("Failed to refresh: {}", e));
            }
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

    /// Get cached project width, computing and caching if needed
    pub fn get_project_width(&mut self) -> u16 {
        if let Some(width) = self.cached_project_width {
            width
        } else {
            use crate::ui::ProjectList;
            let width = ProjectList::max_content_width(&self.projects);
            self.cached_project_width = Some(width);
            width
        }
    }

    /// Get cached session width, computing and caching if needed
    pub fn get_session_width(&mut self) -> u16 {
        if let Some(width) = self.cached_session_width {
            width
        } else {
            use crate::ui::SessionList;
            let width = SessionList::max_content_width(&self.sessions);
            self.cached_session_width = Some(width);
            width
        }
    }

    /// Get cached agent width, computing and caching if needed
    pub fn get_agent_width(&mut self) -> u16 {
        if let Some(width) = self.cached_agent_width {
            width
        } else {
            use crate::ui::AgentList;
            let width = AgentList::max_content_width(&self.agents);
            self.cached_agent_width = Some(width);
            width
        }
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
                buffer: EntryBuffer::new(BUFFER_CAPACITY),
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
                parse_rx,
                parse_tx,
                discovery_rx,
                discovery_tx,
                super_follow_enabled: false,
                cached_project_width: None,
                cached_session_width: None,
                cached_agent_width: None,
                last_conversation_refresh: None,
            }
        })
    }
}
