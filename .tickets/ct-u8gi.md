---
id: ct-u8gi
status: closed
deps: []
links: []
created: 2026-02-06T12:20:57Z
type: task
priority: 2
assignee: Jeffery Utter
parent: ct-mep4
tags: [planned]
---
# Cache max_content_width calculations

max_content_width() for projects, sessions, and agents is called 3 times per frame, iterating all items and computing unicode widths. Cache results and invalidate only when lists change.

## Design

1. Add cached_width: Option<u16> field to App for each list
2. Invalidate cache when list contents change (refresh_projects, load_sessions, load_agents)
3. Return cached value in max_content_width() if available
4. Compute and cache on first call after invalidation

