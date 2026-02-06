---
id: ct-6gwn
status: closed
deps: []
links: []
created: 2026-02-06T12:19:46Z
type: f
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Pretty-format TodoWrite tools

Please pretty-format logs for the TodoWrite tool. If you need an example, check logs in ~/.claude/projects/-home-jeffutter-src-claude-tail/4385e5fe-7fe8-4f6a-ad11-066ffcfd6fd1.jsonl

## Design

### Overview

Add pretty-formatting for TodoWrite tool calls in the conversation view, following the established pattern used by other tools (Bash, Edit, Task, etc.).

### TodoWrite Input Structure

From log analysis, TodoWrite calls have this structure:
```json
{
  "name": "TodoWrite",
  "input": {
    "todos": [
      {
        "content": "Task description",
        "status": "pending" | "in_progress" | "completed",
        "activeForm": "Present continuous form"
      }
    ]
  }
}
```

### Implementation

**File:** `src/ui/conversation.rs`

1. **Add match case** in `render_tool_call()` (around line 61):
   ```rust
   "TodoWrite" => self.render_todowrite_tool(lines, parsed.as_ref(), content_width),
   ```

2. **Create `render_todowrite_tool()` method** following Task/Edit pattern:

   **Collapsed view (header only):**
   ```
   Todo (3): 1 pending, 1 in progress, 1 completed
   ```

   **Expanded view:**
   ```
   Todo (3 items):
     ○ Add helper function
     ◐ Update parser logic
     ✓ Write tests
   ```

3. **Status indicators and colors:**
   | Status | Symbol | Theme Style |
   |--------|--------|-------------|
   | pending | `○` | `tool_input` (gray) |
   | in_progress | `◐` | `tool_name` (yellow bold) |
   | completed | `✓` | `tool_result` (cyan) |

4. **Helper logic:**
   - Parse `todos` array from input JSON
   - Count items by status for header summary
   - Truncate long content strings using `truncate_line()`
   - Use `wrap_text()` for very long item descriptions
   - Indent items with 2 spaces for visual hierarchy

### Code Pattern

```rust
fn render_todowrite_tool(
    &self,
    lines: &mut Vec<Line<'a>>,
    parsed: Option<&serde_json::Value>,
    content_width: usize,
) {
    let todos = parsed
        .and_then(|v| v.get("todos"))
        .and_then(|v| v.as_array());

    let total = todos.map(|t| t.len()).unwrap_or(0);

    // Count by status
    let (pending, in_progress, completed) = count_by_status(todos);

    // Header line
    lines.push(Line::from(vec![
        Span::styled("Todo ", self.theme.tool_name),
        Span::styled(format!("({})", total), self.theme.thinking_collapsed),
        Span::styled(": ", self.theme.tool_name),
        // Status summary...
    ]));

    // Expanded: show each todo item
    if self.expand_tools {
        if let Some(items) = todos {
            for item in items {
                let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("pending");
                let content = item.get("content").and_then(|c| c.as_str()).unwrap_or("");
                let (symbol, style) = match status {
                    "completed" => ("✓", self.theme.tool_result),
                    "in_progress" => ("◐", self.theme.tool_name),
                    _ => ("○", self.theme.tool_input),
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(symbol, style),
                    Span::raw(" "),
                    Span::styled(truncate_line(content, content_width - 6), style),
                ]));
            }
        }
    }
}
```

### Testing

1. Run `cargo check` to verify compilation
2. Run `cargo clippy` for lint checks
3. Manual testing: Open a session with TodoWrite calls and verify:
   - Collapsed view shows count summary
   - Expanded view (press `e`) shows individual items
   - Status colors are correct
   - Long content is properly truncated

### Acceptance Criteria

- [ ] TodoWrite tool calls display with pretty formatting instead of raw JSON
- [ ] Header shows item count and status summary
- [ ] Expanded view lists each todo with status indicator
- [ ] Status-appropriate colors applied (gray/yellow/cyan)
- [ ] Long content truncated gracefully
- [ ] Follows existing code patterns and style conventions
