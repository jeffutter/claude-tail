---
id: ct-mep4
status: open
deps: [ct-zjwp, ct-wy09, ct-u8gi, ct-5z3c, ct-5fww]
links: []
created: 2026-02-06T12:15:38Z
type: bug
priority: 1
assignee: Jeffery Utter
tags: [planned]
---
# Improve UI responsiveness

The UI is still unresponsive. Pressing up/down in the conversation view or tabbing between columns feels like it only re-draws every few seconds. I'm not entirely sure when this gets bad, because sometimes it _is_ responsive. It _might_ be when a session has activity and is updating. It also might be from when the contents of the project lists are updating. Explore these options among others.

## Design

### Root Cause Analysis

Investigation identified **five distinct sources of UI blocking**:

1. **Synchronous incremental parsing** (ct-zjwp) - `refresh_conversation()` calls `parse_jsonl_from_position()` synchronously on the event loop, blocking during file reads when sessions are actively updating.

2. **Synchronous project/session discovery** (ct-wy09) - `refresh_projects()` and `refresh_sessions()` run every 5 seconds, performing O(P×S) blocking file I/O operations to read timestamps from all JSONL files.

3. **Per-frame make_contiguous()** (ct-5z3c) - `app.conversation.make_contiguous()` is called every frame in main.rs:227, potentially causing O(n) reallocation of the entire VecDeque (up to 10k entries).

4. **Per-frame width calculations** (ct-u8gi) - `max_content_width()` is called 3 times per frame, iterating all projects/sessions/agents and computing unicode widths each time.

5. **Full conversation rendering** (ct-5fww) - `render_entries()` generates display lines for ALL entries (up to 10k), then slices for viewport, rather than only processing visible content.

### Why Responsiveness Is Intermittent

The UI feels responsive when:
- No active file updates are occurring
- Project/session lists are small
- Conversation is short

The UI becomes sluggish when:
- A session is actively streaming (triggers incremental parse blocking)
- The 5-second refresh timer fires with many projects/sessions
- Large conversations require expensive per-frame operations

### Execution Order

**Priority 1 (blocking I/O - most impactful):**
1. ct-zjwp: Async incremental parsing - eliminates blocking on active session updates
2. ct-wy09: Async discovery - eliminates 5-second blocking refresh

**Priority 2 (per-frame overhead):**
3. ct-5z3c: Remove make_contiguous() - eliminates O(n) per-frame allocation
4. ct-u8gi: Cache width calculations - reduces per-frame CPU work

**Priority 3 (large conversation optimization):**
5. ct-5fww: Viewport-only rendering - scales to very large conversations

### Verification

After all sub-tickets complete:
1. Test with actively streaming session - should remain responsive
2. Test with 50+ projects - no lag on 5-second refresh
3. Test with 5k+ entry conversation - smooth scrolling
4. Profile CPU usage during idle state - should be minimal

