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
        let home = dirs::home_dir();
        let path_str = self.original_path.to_string_lossy();

        // Replace home directory with ~
        let path_str = if let Some(ref home) = home {
            let home_str = home.to_string_lossy();
            if path_str.starts_with(home_str.as_ref()) {
                format!("~{}", &path_str[home_str.len()..])
            } else {
                path_str.to_string()
            }
        } else {
            path_str.to_string()
        };

        // Abbreviate all components except the last one
        let parts: Vec<&str> = path_str.split('/').collect();
        if parts.len() <= 2 {
            return path_str;
        }

        let abbreviated: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                if i == parts.len() - 1 || part.is_empty() || *part == "~" {
                    part.to_string()
                } else {
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
struct SessionsIndex {
    sessions: HashMap<String, SessionEntry>,
}

#[derive(Debug, Deserialize)]
struct SessionEntry {
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

            let (name, original_path) = decode_project_path(&encoded_path);

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
            for (id, entry) in index.sessions {
                summaries.insert(id, entry.summary);
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
            // Truncate long summaries
            if summary.len() > 40 {
                format!("{}... ({})", &summary[..37], timestamp)
            } else {
                format!("{} ({})", summary, timestamp)
            }
        } else {
            format!("{} ({})", self.short_id(), timestamp)
        }
    }
}
