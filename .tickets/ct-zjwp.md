---
id: ct-zjwp
status: open
deps: []
links: []
created: 2026-02-06T12:20:33Z
type: task
priority: 1
assignee: Jeffery Utter
parent: ct-mep4
tags: [planned]
---
# Move incremental JSONL parsing to background thread

refresh_conversation() calls parse_jsonl_from_position() synchronously on the main event loop, blocking UI during file reads. Move this to tokio::task::spawn_blocking like the initial parse already does.

## Design

1. In app.rs refresh_conversation(), spawn parse_jsonl_from_position_async() instead of sync version
2. Send result via parse_rx channel (reuse existing ParseMessage pattern)
3. Handle incremental parse completion in handle_parse_complete()
4. Track 'is_refreshing' state to prevent duplicate refreshes

