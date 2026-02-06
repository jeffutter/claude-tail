---
id: ct-pcrq
status: open
deps: []
links: []
created: 2026-02-06T04:20:28Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned]
design: |
  ## Approach

  Follow the existing Session/Agent pattern exactly. Projects will gain a `last_modified`
  field derived from the most recent session within each project, then sort and display
  using the same conventions already used by sessions and agents.

  ## Changes

  ### 1. Add `last_modified` to `Project` struct (`src/logs/project.rs:11-17`)

  Add a `last_modified: std::time::SystemTime` field to the `Project` struct.

  Add methods matching the Session/Agent pattern:
  - `timestamp_str() -> String` — formats as `HH:MM:SS` via `chrono::DateTime<Local>`
    (identical to `Session::timestamp_str()` at line 246 and `Agent::timestamp_str()` at types.rs:161)
  - `display_name_with_timestamp() -> String` — returns `"abbreviated_path (HH:MM:SS)"`

  ### 2. Compute `last_modified` during discovery (`src/logs/project.rs:118-164`)

  In `discover_projects()`, after collecting each project directory, determine its
  `last_modified` by scanning session JSONL files within the project directory and
  taking the maximum file mtime. This avoids calling the full `discover_sessions()`
  (which does more work than needed) — just iterate the directory entries and read
  `metadata().modified()`.

  If a project has no sessions, fall back to the project directory's own mtime.

  ### 3. Sort projects by `last_modified` descending (`src/logs/project.rs:162`)

  Replace the current alphabetical sort:
  ```rust
  projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  ```
  with:
  ```rust
  projects.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
  ```
  This matches sessions (line 227) and agents (line 332).

  ### 4. Display timestamp in project list (`src/ui/project_list.rs:60-77`)

  Update the list item rendering to include the timestamp after the abbreviated path,
  matching how sessions and agents display:
  ```
  > ~/s/c/my-project (14:32:05)
  ```

  Update `max_content_width()` to account for the added timestamp text width
  (the ` (HH:MM:SS)` suffix adds 11 characters).

  ## Files Modified

  - `src/logs/project.rs` — struct, methods, discovery, sorting
  - `src/ui/project_list.rs` — display rendering, width calculation

  ## No New Dependencies

  `chrono` is already in Cargo.toml and `std::time::SystemTime` is stdlib.
---
# Sort project list by last updated

Similar to the other lists, show the last updated time for every project entry and sort the list with the most recent activity first

