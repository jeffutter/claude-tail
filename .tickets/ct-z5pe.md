---
id: ct-z5pe
status: closed
deps: []
links: []
created: 2026-02-06T12:07:02Z
type: bug
priority: 2
assignee: Jeffery Utter
tags: [planned]
design: |
  ## Analysis

  The ct-pcrq implementation is structurally correct but uses the wrong timestamp source.
  All three discovery functions (projects, sessions, agents) use **file system mtime**
  instead of the **actual timestamps from JSONL content**.

  **Current behavior:**
  - `compute_project_last_modified()` (line 132): Uses `metadata.modified()` on JSONL files
  - `discover_sessions()` (line 257): Uses `metadata.modified()` on session JSONL
  - `discover_agents()` (line 337, 362): Uses `metadata.modified()` on agent JSONL

  **Expected behavior:**
  JSONL entries contain `timestamp: Option<DateTime<Utc>>` (types.rs:13). The "last activity"
  should be the timestamp of the **last entry** in each JSONL file, not the file's mtime.

  ## Approach

  Create a helper function to extract the last timestamp from a JSONL file by reading
  the last line and parsing its timestamp field. This avoids parsing the entire file.

  ### 1. Add `get_last_jsonl_timestamp()` helper (`src/logs/project.rs`)

  ```rust
  use chrono::{DateTime, Utc};

  /// Extracts the timestamp of the last entry in a JSONL file.
  /// Falls back to file mtime if parsing fails or no timestamp exists.
  fn get_last_jsonl_timestamp(path: &Path) -> SystemTime {
      // Read file and find last non-empty line
      // Parse as JSON, extract "timestamp" field
      // Convert DateTime<Utc> to SystemTime
      // Fall back to metadata.modified() on failure
  }
  ```

  Implementation notes:
  - Read file to string, split by lines, find last non-empty line
  - Parse with `serde_json::from_str::<LogEntry>(line)`
  - Extract `entry.timestamp` and convert to `SystemTime`
  - Fallback chain: timestamp → file mtime → UNIX_EPOCH

  ### 2. Update `compute_project_last_modified()` (lines 132-158)

  Replace direct mtime usage with `get_last_jsonl_timestamp()`:
  ```rust
  if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
      let modified = get_last_jsonl_timestamp(&path);
      if modified > max_modified {
          max_modified = modified;
      }
  }
  ```

  ### 3. Update `discover_sessions()` (lines 256-259)

  Replace:
  ```rust
  let metadata = entry.metadata()?;
  let last_modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
  ```
  With:
  ```rust
  let last_modified = get_last_jsonl_timestamp(&path);
  ```

  ### 4. Update `discover_agents()` (lines 336-338, 361-364)

  For main agent (line 336-338):
  ```rust
  let main_modified = get_last_jsonl_timestamp(&session.log_path);
  ```

  For sub-agents (line 361-364):
  ```rust
  let modified = get_last_jsonl_timestamp(&path);
  ```

  ## Files Modified

  - `src/logs/project.rs` — Add helper function, update three discovery functions

  ## Performance Consideration

  Reading the last line of each JSONL file adds I/O overhead during discovery. This is
  acceptable because:
  1. Discovery runs infrequently (startup + file change events)
  2. Files are typically small (conversation logs)
  3. Accuracy of timestamps is more important than discovery speed

  If performance becomes an issue, consider caching timestamps or reading only
  the last N bytes of each file to find the final entry.

  ## Testing

  1. Verify projects sort correctly by most recent session activity
  2. Verify sessions sort correctly by most recent agent activity
  3. Verify agents sort correctly by most recent entry
  4. Verify timestamps display correctly in all three lists
  5. Test fallback behavior when JSONL has no timestamps or is empty
---
# Ensure project list is sorted by most recent activity

I'm not sure the work done in ct-pcrq was completed correctly. I don't see the most recent modified time after the project path in the list (like we do for sessions and agents), also the lists do not appear to be sorted. Lastly, looking at the code I think we're taking the modified timestamp of the session files, rather than reading it from the JSONL of the session itself. The time of the project should be the time of the most recent session which should be the time of the most recent agent.

