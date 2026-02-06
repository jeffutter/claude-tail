---
id: ct-wy09
status: open
deps: []
links: []
created: 2026-02-06T12:20:46Z
type: task
priority: 1
assignee: Jeffery Utter
parent: ct-mep4
---
# Move project/session discovery to background threads

refresh_projects() and refresh_sessions() run synchronously every 5 seconds, performing O(P×S) file I/O operations that block the event loop. Move discovery to background threads.

