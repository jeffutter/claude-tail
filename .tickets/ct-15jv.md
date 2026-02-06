---
id: ct-15jv
status: closed
deps: []
links: []
created: 2026-02-06T04:19:33Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Add compressed project path to header

In the application header, show the compressed style project path, rather than the entire path

## Design

### Problem

The header currently displays the full project path via `App::selected_project_path()` (e.g. `~/src/claude-tail`), while the project list already uses `Project::abbreviated_path()` which compresses intermediate components to single letters (e.g. `~/s/claude-tail`). The header should match the compressed style for consistency and to save horizontal space.

### Approach

Replace `app.selected_project_path()` with a new method that delegates to `Project::abbreviated_path()`.

### Changes

**`src/app.rs`** — Add a new method `selected_project_abbreviated_path()`:

```rust
pub fn selected_project_abbreviated_path(&self) -> Option<String> {
    self.project_state
        .selected()
        .and_then(|idx| self.projects.get(idx))
        .map(|p| p.abbreviated_path())
}
```

This parallels the existing `selected_project_path()` and `selected_project_name()` methods.

**`src/main.rs`** — In `draw_header()` (line 244), change:

```rust
// Before:
let project_path = app.selected_project_path().unwrap_or_default();

// After:
let project_path = app.selected_project_abbreviated_path().unwrap_or_default();
```

### Notes

- `Project::abbreviated_path()` already exists at `src/logs/project.rs:21-85` and is used by the project list widget.
- No new dependencies or test changes required.
- `selected_project_path()` can be left in place or removed — it has no other callers currently, but removing it is optional cleanup.

## Notes

**2026-02-06T04:50:18Z**

Removed unused selected_project_path() method as suggested by code review for codebase cleanliness
