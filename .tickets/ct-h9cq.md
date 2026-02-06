---
id: ct-h9cq
status: open
deps: []
links: []
created: 2026-02-06T18:35:28Z
type: feature
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Right align timestamps

Right align the timestamps in the project/session/agent columns

## Design

### Approach: Multi-Span Lines with Calculated Padding

Right-align timestamps by splitting each list item's `Line` into multiple `Span`s — left-aligned label text, space padding, and a right-aligned timestamp — instead of the current single concatenated string.

### Current State

All three list panes (`project_list.rs`, `session_list.rs`, `agent_list.rs`) construct list items as a single `Span` containing the concatenated display name + timestamp:

```rust
ListItem::new(Line::from(vec![Span::styled(
    format!("{}{}", prefix, project.display_name_with_timestamp()),
    style,
)]))
```

Timestamps are appended inline: `~/s/c/project-name (14:32:05)`.

### Target State

Each list item becomes multiple `Span`s within a `Line`:

```rust
ListItem::new(Line::from(vec![
    Span::styled(format!("{}{}", prefix, name), style),
    Span::styled(" ".repeat(padding), Style::default()),
    Span::styled(format!("({})", timestamp), theme.timestamp),
]))
```

The padding is calculated per-item based on the available pane width, the label width, and the timestamp width, so timestamps align flush-right.

### Files to Modify

1. **`src/logs/project.rs`** — Add `abbreviated_path()` as a public method (or ensure it's accessible without the timestamp). The existing `display_name_with_timestamp()` concatenates name + timestamp; we need them separate. Add a method like `display_name_without_timestamp()` that returns just the abbreviated path. Keep `display_name_with_timestamp()` for backward compatibility with `max_content_width()`.

2. **`src/logs/types.rs`** — Same for `Agent`: expose `display_name` and `timestamp_str()` separately so the UI can position them independently. `Agent.display_name` is already public; `timestamp_str()` is also public. No changes needed here.

3. **`src/ui/project_list.rs`** — Modify `render()` (around line 60-78):
   - Calculate available inner width from the render `area` (subtract borders/padding: `area.width - 2`)
   - For each project, build a multi-span `Line`:
     - `Span` 1: `prefix + project.abbreviated_path()` with item style
     - `Span` 2: padding spaces (calculated: `available_width - label_width - timestamp_width`)
     - `Span` 3: `(HH:MM:SS)` with `theme.timestamp` style
   - Use `saturating_sub` to handle cases where the pane is too narrow (timestamp gets clipped naturally by ratatui)

4. **`src/ui/session_list.rs`** — Same pattern. Sessions already call `session.display_name()` which includes the timestamp inline. Refactor to separate the name text from the timestamp, then build multi-span lines. Session's `display_name()` in `project.rs` (around line 366) concatenates summary + timestamp — split this so the UI controls positioning.

5. **`src/ui/agent_list.rs`** — Same pattern. Use `agent.display_name` for label and `agent.timestamp_str()` for the right-aligned timestamp. Preserve the `theme.title` styling for the main agent's label span.

6. **`src/ui/project_list.rs`, `session_list.rs`, `agent_list.rs` — `max_content_width()`** — These may need adjustment. Currently they measure the full concatenated string. With right-alignment, the width calculation should still account for the full content (name + gap + timestamp) to ensure the pane is wide enough. The existing calculation should remain correct since `display_name_with_timestamp()` already includes both parts.

### Implementation Notes

- **Width source**: The render `area` parameter already provides the available width. Use `area.width.saturating_sub(2)` to account for borders.
- **Existing theme field**: `theme.timestamp` (defined in `styles.rs` line 23) already exists but is unused in list rendering. Use it for timestamp spans to give timestamps a distinct dimmer color.
- **Unicode width**: Continue using `unicode_width::UnicodeWidthStr` for calculating padding — already imported in all three files.
- **Edge case — narrow panes**: When the pane is too narrow for padding, use `saturating_sub` so padding becomes 0 and the timestamp sits immediately after the name. Ratatui will clip overflow naturally.
- **Cache invalidation**: The cached width calculations (`cached_project_width`, etc.) should not need changes since `max_content_width()` still measures the full content width.
- **Session refactor**: `Session::display_name()` currently bakes the timestamp into the string. Add a `display_name_without_timestamp()` method (or rename/refactor) so the UI can control timestamp placement.

### Testing

- Verify all three panes show timestamps right-aligned when focused
- Check behavior when pane width is very narrow (collapsed panes already show single-letter indicators, so this only matters for focused/semi-focused widths)
- Confirm timestamp styling uses `theme.timestamp` color
- Test with long project paths, long session summaries, and agent names to ensure padding calculates correctly
- Verify the main agent still gets `theme.title` styling on its label

