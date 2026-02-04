use anyhow::Result;
use chrono::{DateTime, Local};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub encoded_path: String,
    pub original_path: PathBuf,
}

impl Project {
    /// Returns an abbreviated path like `~/s/c/my-project`
    pub fn abbreviated_path(&self) -> String {
        let path_str = self.original_path.to_string_lossy();

        let home = dirs::home_dir();
        let home_str = home.as_ref()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_default();

        // Make path relative to home
        let relative_path = if !home_str.is_empty() && path_str.starts_with(&home_str) {
            // Direct match - path starts with home directory
            format!("~{}", &path_str[home_str.len()..])
        } else if !home_str.is_empty() && (path_str.starts_with("/Users/") || path_str.starts_with("/home/")) {
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
                let remaining: Vec<&str> = path_parts.into_iter().skip(home_encoded_components).collect();
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
                    part.chars().next().map(|c| c.to_string()).unwrap_or_default()
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

pub fn discover_projects() -> Result<Vec<Project>> {
    let projects_dir = get_claude_projects_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

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
            let original_path = load_original_path(&sessions_index_path)
                .unwrap_or_else(|| {
                    // Fallback to decoding if sessions-index.json doesn't have it
                    let (_, decoded) = decode_project_path(&encoded_path);
                    decoded
                });

            let name = original_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&encoded_path)
                .to_string();

            projects.push(Project {
                name,
                path,
                encoded_path,
                original_path,
            });
        }
    }

    projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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

            let metadata = entry.metadata()?;
            let last_modified = metadata.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);

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

    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(index) = serde_json::from_str::<SessionsIndex>(&content) {
            for entry in index.entries {
                summaries.insert(entry.session_id, entry.summary);
            }
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
