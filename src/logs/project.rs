use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;

use super::types::Agent;

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub encoded_path: String,
    pub original_path: PathBuf,
    pub last_modified: SystemTime,
}

impl Project {
    /// Returns the timestamp formatted as HH:MM:SS
    pub fn timestamp_str(&self) -> String {
        let datetime: DateTime<Local> = self.last_modified.into();
        datetime.format("%H:%M:%S").to_string()
    }

    /// Returns display string with timestamp: "abbreviated_path (HH:MM:SS)"
    pub fn display_name_with_timestamp(&self) -> String {
        format!("{} ({})", self.abbreviated_path(), self.timestamp_str())
    }

    /// Returns an abbreviated path like `~/s/c/my-project`
    pub fn abbreviated_path(&self) -> String {
        let path_str = self.original_path.to_string_lossy();

        let home = dirs::home_dir();
        let home_str = home
            .as_ref()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_default();

        // Make path relative to home
        let relative_path = if !home_str.is_empty() && path_str.starts_with(&home_str) {
            // Direct match - path starts with home directory
            format!("~{}", &path_str[home_str.len()..])
        } else if !home_str.is_empty()
            && (path_str.starts_with("/Users/") || path_str.starts_with("/home/"))
        {
            // Home directory might be split due to encoding (e.g., jeffery.utter -> jeffery/utter)
            // Count components in actual home, then skip that many "encoded" components
            // accounting for dots that became extra path separators
            let home_parts: Vec<&str> = home_str.split('/').filter(|s| !s.is_empty()).collect();
            let path_parts: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();

            // Count how many dots are in the home path (each becomes an extra separator when encoded)
            let extra_separators: usize = home_parts.iter().map(|p| p.matches('.').count()).sum();
            let home_encoded_components = home_parts.len() + extra_separators;

            // Skip that many components from the path
            if path_parts.len() > home_encoded_components {
                let remaining: Vec<&str> = path_parts
                    .into_iter()
                    .skip(home_encoded_components)
                    .collect();
                format!("~/{}", remaining.join("/"))
            } else {
                "~".to_string()
            }
        } else {
            path_str.to_string()
        };

        // Abbreviate intermediate components (keep first ~ and last component full)
        let parts: Vec<&str> = relative_path.split('/').collect();
        if parts.len() <= 2 {
            return relative_path;
        }

        let abbreviated: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                if i == 0 || i == parts.len() - 1 || part.is_empty() {
                    // Keep ~ (first), last component, and empty parts as-is
                    part.to_string()
                } else {
                    // Abbreviate intermediate components
                    part.chars()
                        .next()
                        .map(|c| c.to_string())
                        .unwrap_or_default()
                }
            })
            .collect();

        abbreviated.join("/")
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub project_path: PathBuf,
    pub log_path: PathBuf,
    pub summary: Option<String>,
    pub last_modified: std::time::SystemTime,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionsIndex {
    #[serde(default)]
    original_path: Option<String>,
    #[serde(default)]
    entries: Vec<SessionIndexEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionIndexEntry {
    session_id: String,
    #[serde(default)]
    summary: Option<String>,
}

pub fn get_claude_projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude").join("projects"))
}

/// Extracts the timestamp of the last entry in a JSONL file.
/// Falls back to file mtime if parsing fails or no timestamp exists.
fn get_last_jsonl_timestamp(path: &Path) -> SystemTime {
    // Try to read the file and parse the last entry's timestamp
    if let Ok(content) = std::fs::read_to_string(path) {
        // Find the last non-empty line
        if let Some(last_line) = content.lines().rfind(|l| !l.trim().is_empty()) {
            // Try to parse as a minimal JSON object with just the timestamp field
            #[derive(Deserialize)]
            struct TimestampOnly {
                timestamp: Option<DateTime<Utc>>,
            }

            if let Ok(entry) = serde_json::from_str::<TimestampOnly>(last_line) {
                // Extract timestamp and convert to SystemTime
                if let Some(timestamp) = entry.timestamp {
                    // Convert DateTime<Utc> to SystemTime
                    let duration_since_epoch =
                        timestamp.signed_duration_since(DateTime::UNIX_EPOCH);
                    if let Ok(std_duration) = duration_since_epoch.to_std() {
                        return SystemTime::UNIX_EPOCH + std_duration;
                    }
                }
            }
        }
    }

    // Fallback to file mtime
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Computes the last modified time for a project by finding the most recent
/// session JSONL file. Falls back to the project directory's mtime if no sessions exist.
fn compute_project_last_modified(project_path: &Path) -> SystemTime {
    let mut max_modified = SystemTime::UNIX_EPOCH;

    // Scan for JSONL files in the project directory
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let modified = get_last_jsonl_timestamp(&path);
                if modified > max_modified {
                    max_modified = modified;
                }
            }
        }
    }

    // If no sessions found, use the project directory's mtime
    if max_modified == SystemTime::UNIX_EPOCH
        && let Ok(metadata) = std::fs::metadata(project_path)
        && let Ok(modified) = metadata.modified()
    {
        return modified;
    }

    max_modified
}

pub fn discover_projects() -> Result<Vec<Project>> {
    let projects_dir = get_claude_projects_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();

    for entry in std::fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let encoded_path = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            // Try to get the original path from sessions-index.json
            let sessions_index_path = path.join("sessions-index.json");
            let original_path = load_original_path(&sessions_index_path).unwrap_or_else(|| {
                // Fallback to decoding if sessions-index.json doesn't have it
                let (_, decoded) = decode_project_path(&encoded_path);
                decoded
            });

            let name = original_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&encoded_path)
                .to_string();

            // Compute last_modified from session JSONL files
            let last_modified = compute_project_last_modified(&path);

            projects.push(Project {
                name,
                path,
                encoded_path,
                original_path,
                last_modified,
            });
        }
    }

    // Sort by last modified, newest first
    projects.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(projects)
}

/// Loads the original path from sessions-index.json if available
fn load_original_path(sessions_index_path: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(sessions_index_path).ok()?;
    let index: SessionsIndex = serde_json::from_str(&content).ok()?;
    index.original_path.map(PathBuf::from)
}

/// Decodes the encoded project path and returns (name, original_path)
fn decode_project_path(encoded: &str) -> (String, PathBuf) {
    // Claude encodes paths like "-Users-username-src-project"
    // The leading dash represents the root /
    let path_str = encoded.replace('-', "/");
    let original_path = PathBuf::from(&path_str);

    // Extract just the last component as the display name
    let name = original_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(encoded)
        .to_string();

    (name, original_path)
}

pub fn discover_sessions(project: &Project) -> Result<Vec<Session>> {
    let sessions_index_path = project.path.join("sessions-index.json");
    let mut sessions = Vec::new();

    // Load session summaries if available
    let summaries = load_session_summaries(&sessions_index_path);

    // Find all JSONL files in the project directory
    for entry in std::fs::read_dir(&project.path)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let last_modified = get_last_jsonl_timestamp(&path);

            let summary = summaries.get(&session_id).cloned().flatten();

            sessions.push(Session {
                id: session_id,
                project_path: project.path.clone(),
                log_path: path,
                summary,
                last_modified,
            });
        }
    }

    // Sort by last modified, newest first
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(sessions)
}

fn load_session_summaries(path: &Path) -> HashMap<String, Option<String>> {
    let mut summaries = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(path)
        && let Ok(index) = serde_json::from_str::<SessionsIndex>(&content)
    {
        for entry in index.entries {
            summaries.insert(entry.session_id, entry.summary);
        }
    }

    summaries
}

impl Session {
    /// Returns the timestamp formatted as HH:MM:SS
    pub fn timestamp_str(&self) -> String {
        let datetime: DateTime<Local> = self.last_modified.into();
        datetime.format("%H:%M:%S").to_string()
    }

    /// Returns the session ID (possibly truncated)
    pub fn short_id(&self) -> String {
        if self.id.len() > 8 {
            format!("{}...", &self.id[..8])
        } else {
            self.id.clone()
        }
    }

    /// Returns display name with timestamp: "summary (HH:MM:SS)" or "id... (HH:MM:SS)"
    pub fn display_name(&self) -> String {
        let timestamp = self.timestamp_str();
        if let Some(ref summary) = self.summary {
            // Truncate long summaries (respecting char boundaries)
            if summary.len() > 40 {
                let truncate_at = summary
                    .char_indices()
                    .take_while(|(i, _)| *i < 37)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(summary.len());
                format!("{}... ({})", &summary[..truncate_at], timestamp)
            } else {
                format!("{} ({})", summary, timestamp)
            }
        } else {
            format!("{} ({})", self.short_id(), timestamp)
        }
    }
}

/// Discovers all agents for a session.
/// Returns a list with "Main" agent first, followed by sub-agents sorted by most recent activity.
pub fn discover_agents(session: &Session) -> Result<Vec<Agent>> {
    let mut agents = Vec::new();

    // Always include the main agent from the session's log file
    let main_modified = get_last_jsonl_timestamp(&session.log_path);

    agents.push(Agent {
        id: "main".to_string(),
        display_name: "Main".to_string(),
        log_path: session.log_path.clone(),
        last_modified: main_modified,
        is_main: true,
    });

    // Check for subagents directory: {session_id}/subagents/
    let subagents_dir = session.project_path.join(&session.id).join("subagents");

    if subagents_dir.is_dir() {
        for entry in std::fs::read_dir(&subagents_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Look for agent-*.jsonl files
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && let Some(filename) = path.file_stem().and_then(|s| s.to_str())
                && let Some(agent_info) = parse_agent_filename(filename)
            {
                let modified = get_last_jsonl_timestamp(&path);

                agents.push(Agent {
                    id: agent_info.id,
                    display_name: agent_info.display_name,
                    log_path: path,
                    last_modified: modified,
                    is_main: false,
                });
            }
        }
    }

    // Sort: Main first (already at index 0), then sub-agents by last_modified (newest first)
    if agents.len() > 1 {
        agents[1..].sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    }

    Ok(agents)
}

struct AgentInfo {
    id: String,
    display_name: String,
}

/// Parses agent filename to extract ID and display name.
/// Supports formats like:
/// - "agent-a356e17" -> id: "a356e17", display: "a356e17"
/// - "agent-Explore-a356e17" -> id: "a356e17", display: "Explore"
fn parse_agent_filename(filename: &str) -> Option<AgentInfo> {
    if !filename.starts_with("agent-") {
        return None;
    }

    let rest = &filename[6..]; // Skip "agent-"

    // Check if there's a type prefix (e.g., "Explore-a356e17")
    if let Some(dash_pos) = rest.rfind('-') {
        let potential_type = &rest[..dash_pos];
        let id = &rest[dash_pos + 1..];

        // If the prefix looks like a type (contains letters and isn't just the ID)
        if !potential_type.is_empty()
            && potential_type.chars().any(|c| c.is_alphabetic())
            && !id.is_empty()
        {
            return Some(AgentInfo {
                id: id.to_string(),
                display_name: potential_type.to_string(),
            });
        }
    }

    // No type prefix, just "agent-{id}"
    Some(AgentInfo {
        id: rest.to_string(),
        display_name: rest.to_string(),
    })
}
