pub mod buffer;
pub mod index;
pub mod parser;
pub mod project;
pub mod types;
pub mod watcher;

pub use buffer::EntryBuffer;
pub use parser::{ParseResult, parse_jsonl_range};
pub use project::{Project, Session, discover_agents, discover_projects, discover_sessions};
pub use types::{Agent, DisplayEntry, ToolCallResult};
pub use watcher::{SessionWatcher, WatcherEvent};
