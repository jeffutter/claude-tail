---
id: ct-a9d4
status: open
deps: []
links: []
created: 2026-02-06T04:51:10Z
type: bug
priority: 1
assignee: Jeffery Utter
tags: [needs-plan]
---
# Fix hanging when switching sessions

It seems that sometimes when switching sessions the application hangs for either a short time or potentially a very long time. It sort of seems like it's loading some very large data into memory. Are we possibly loading sessions other than just the one being viewed?

