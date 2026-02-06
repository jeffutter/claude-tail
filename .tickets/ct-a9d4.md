---
id: ct-a9d4
status: open
deps: [ct-r7by, ct-ywkl]
links: []
created: 2026-02-06T04:51:10Z
type: bug
priority: 1
assignee: Jeffery Utter
tags: [planned]
design: |
  ## Root Cause

  When switching sessions, `load_conversation_for_selected_agent()` (app.rs:263-304) calls
  `parse_jsonl_file()` (parser.rs:18-66) synchronously on the main tokio event loop thread.
  This reads the entire JSONL file into memory via `file.read_to_string()`, parses every line
  with serde_json, runs `merge_tool_results()` (which clones all entries), and converts to
  VecDeque—all blocking the UI. For large conversations (100KB-1MB+), this causes 200ms-2s+
  freezes.

  The app is NOT loading other sessions' data—it's just that the selected session's data is
  loaded synchronously and blocks the entire event loop.

  ### Blocking call chain during session switch

  ```
  input/handler.rs:91-92
    → app.load_agents_for_selected_session()     [sync fs::read_dir + metadata]
    → app.load_conversation_for_selected_agent()  [sync file read + parse + merge]
      → parser.rs:21  file.read_to_string()       [BLOCKS: full file I/O]
      → parser.rs:29  content.lines() + serde      [BLOCKS: CPU-bound parse]
      → parser.rs:445 merge_tool_results()         [BLOCKS: clones all entries]
      → app.rs:283    VecDeque::from(merged)        [BLOCKS: reallocation]
  ```

  ## Fix Strategy

  Two sub-tickets address this:

  1. **ct-r7by**: Move parsing to a background thread via `tokio::spawn_blocking()`.
     The main event loop should remain responsive while parsing completes. This is the
     critical fix that eliminates the freeze entirely.

  2. **ct-ywkl**: Optimize `merge_tool_results()` to avoid cloning every entry. This
     reduces CPU time during the merge step (50-150ms savings on large conversations).
     Independent of ct-r7by—can be done before or after.

  ## Verification

  After both sub-tickets are complete:
  - Test with large JSONL files (1MB+) to confirm no UI freeze
  - Verify conversation content still displays correctly after async load
  - Confirm file watcher still works (stop/start around load)
  - Test rapid session switching (pressing j/k quickly) doesn't cause races
---
# Fix hanging when switching sessions

It seems that sometimes when switching sessions the application hangs for either a short time or potentially a very long time. It sort of seems like it's loading some very large data into memory. Are we possibly loading sessions other than just the one being viewed?

