---
id: ct-r7by
status: closed
deps: []
links: []
created: 2026-02-06T04:57:24Z
type: task
priority: 1
assignee: Jeffery Utter
parent: ct-a9d4
---
# Move JSONL parsing to background thread with tokio::spawn_blocking

The primary cause of UI hangs when switching sessions is that parse_jsonl_file() in parser.rs reads the entire file into memory and parses every line synchronously on the main tokio event loop thread. This blocks all rendering and input handling. Move the parsing to a background thread using tokio::spawn_blocking(), send results back via a channel or task handle, and update the UI when parsing completes. While parsing is in progress, the UI should remain responsive (show a loading indicator or the previous conversation). Key files: app.rs (load_conversation_for_selected_agent), parser.rs (parse_jsonl_file), main.rs (event loop).


## Notes

**2026-02-06T05:03:37Z**

Implementation complete and reviewed. Code reviewer identified optimization opportunities for future work: 1) Add task cancellation to abort previous parse tasks when switching sessions rapidly, 2) Add generation counter for clearer staleness tracking, 3) Consider timeout for large file protection. Current implementation is production-ready and functionally correct.
