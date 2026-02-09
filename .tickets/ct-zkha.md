---
id: ct-zkha
status: closed
deps: []
links: []
created: 2026-02-09T05:21:28Z
type: feature
priority: 2
assignee: Jeffery Utter
---
# Virtual scrolling / pagination for JSONL log viewing

Only parse and hold JSONL entries needed for display using a line-offset index for random access. Async scroll loading keeps UI responsive during rapid paging. Sync tail updates for live following.

## Design

### Virtual Scrolling / Pagination for JSONL Log Viewing

#### Context

Currently, opening a conversation parses the entire JSONL file into a `VecDeque<DisplayEntry>` (up to 10,000 entries). Even though rendering already virtualizes (only renders visible entries), the full file must be read, parsed, and held in memory. This is wasteful for large files and adds latency on agent switch. The goal: only parse and hold the entries needed for display, using a line-offset index for random access.

#### Architecture Overview

Two new modules, one modified module:

```
src/logs/
    index.rs     NEW  - JSONL line byte-offset index (fast newline scan, no parsing)
    buffer.rs    NEW  - Windowed entry buffer with demand-driven loading
    parser.rs    MOD  - Add parse_jsonl_range() for bounded byte-range parsing
```

The rest of the system talks to `EntryBuffer` through a small interface. Rendering code receives a `&VecDeque<DisplayEntry>` exactly as today.

```
File ─→ LineIndex (byte offsets) ─→ EntryBuffer (windowed VecDeque<DisplayEntry>)
              ↑                            ↑          ↓
         Watcher events              App scroll    Rendering
                                     commands      (unchanged)
```

#### Phase 1: LineIndex (`src/logs/index.rs`)

A byte-level index of JSONL line start positions. Scans raw bytes for `
` — no JSON parsing.

```rust
pub struct LineIndex {
    /// Byte offset where each JSONL line starts. offsets[0] = 0.
    offsets: Vec<u64>,
    /// File size at last index time.
    indexed_bytes: u64,
}
```

**Interface:**
- `LineIndex::build(path) -> Result<Self>` — full scan via BufReader in 64KB chunks
- `index.update(path) -> Result<usize>` — seek to `indexed_bytes`, scan new bytes, return count of new lines
- `index.line_count() -> usize`
- `index.line_byte_range(line) -> Option<(u64, u64)>` — byte range for one line
- `index.range_byte_range(start, end) -> Option<(u64, u64)>` — byte range for `[start..end)` lines
- `index.indexed_bytes() -> u64`

**Edge cases:**
- Empty file: 0 lines, 0 bytes
- File truncation: `update()` detects `file_size < indexed_bytes`, triggers full re-index
- Partial line at EOF: the offset is recorded but parsing will handle the incomplete JSON gracefully (existing EOF behavior)
- Unicode: scanning for byte `0x0A` is safe in UTF-8

**Tests:** empty file, single line, multi-line, incremental update, file truncation detection, no trailing newline.

#### Phase 2: Range Parsing (`src/logs/parser.rs`)

Add a function to parse a specific byte range from a file:

```rust
/// Parse JSONL entries from bytes [start..end) of a file.
pub fn parse_jsonl_range(path: &Path, start: u64, end: u64) -> Result<ParseResult>

pub async fn parse_jsonl_range_async(path: PathBuf, start: u64, end: u64) -> Result<ParseResult>
```

Implementation: open file, seek to `start`, read `(end - start)` bytes into a String, call existing `parse_stream_content(&content, start)`. The existing function handles all parsing, error recovery, and position tracking.

**Tests:** parse middle of file, parse single line, parse last line, boundary alignment with LineIndex.

#### Phase 3: EntryBuffer (`src/logs/buffer.rs`)

The core abstraction. Manages a windowed `VecDeque<DisplayEntry>` backed by the `LineIndex`.

```rust
pub struct EntryBuffer {
    index: LineIndex,
    entries: VecDeque<DisplayEntry>,
    /// JSONL line index of the first entry in the buffer
    window_start_line: usize,
    /// JSONL line index of the last entry in the buffer (inclusive)
    window_end_line: usize,
    /// Max JSONL lines to keep parsed in the buffer
    capacity: usize,  // default 100
    path: PathBuf,
    parse_errors: Vec<String>,
    /// In-flight async load request (for scroll-triggered loads)
    pending_load: Option<PendingLoad>,
}

struct PendingLoad {
    /// Target JSONL line range being loaded
    target_start: usize,
    target_end: usize,
    /// Whether this load prepends (older) or replaces/appends
    direction: LoadDirection,
}

enum LoadDirection {
    Older,   // prepending older entries
    Newer,   // appending newer entries
    Replace, // replacing buffer (jump to start/end)
}
```

**Interface:**

```rust
impl EntryBuffer {
    pub fn new(capacity: usize) -> Self;

    // === Synchronous operations (fast, always immediate) ===

    /// Load a new file. Builds index, loads tail entries (for follow mode start).
    /// Synchronous — index scan is microseconds, parsing ≤100 lines is <5ms.
    pub fn load_file(&mut self, path: &Path) -> Result<()>;

    /// File changed (watcher event). Updates index.
    /// If follow_mode, parses and appends new entries, evicts old from front.
    /// Returns count of new entries added.
    /// Synchronous — tail updates parse only the few new lines.
    pub fn file_changed(&mut self, follow_mode: bool) -> Result<usize>;

    /// Access the current entries for rendering.
    pub fn entries(&self) -> &VecDeque<DisplayEntry>;

    /// Whether there are older entries available beyond the buffer.
    pub fn has_older(&self) -> bool;

    /// Whether there are newer entries available beyond the buffer.
    pub fn has_newer(&self) -> bool;

    /// Total JSONL lines in the file (for approximate scrollbar).
    pub fn total_file_lines(&self) -> usize;

    /// Current window position as (start_line, end_line) for scrollbar.
    pub fn window_position(&self) -> (usize, usize);

    /// Whether an async load is in flight.
    pub fn is_loading(&self) -> bool;

    /// Parse errors encountered.
    pub fn parse_errors(&self) -> &[String];

    // === Async operations (for scroll-triggered loading) ===

    /// Request loading older entries. Returns None if already loading or nothing to load.
    /// Returns Some((path, byte_start, byte_end)) for the caller to spawn a parse task.
    pub fn request_load_older(&mut self, count: usize) -> Option<(PathBuf, u64, u64)>;

    /// Request loading newer entries.
    pub fn request_load_newer(&mut self, count: usize) -> Option<(PathBuf, u64, u64)>;

    /// Request jump to file start. Returns parse parameters.
    pub fn request_jump_to_start(&mut self) -> Option<(PathBuf, u64, u64)>;

    /// Request jump to file end. Returns parse parameters.
    pub fn request_jump_to_end(&mut self) -> Option<(PathBuf, u64, u64)>;

    /// Receive results from an async parse. Updates buffer, returns
    /// (added_rendered_lines, evicted_rendered_lines) for scroll_offset adjustment.
    /// content_width needed to calculate rendered line counts.
    pub fn receive_loaded(
        &mut self,
        result: Result<ParseResult>,
        content_width: usize,
    ) -> (usize, usize);
}
```

**Async scroll loading flow:**
1. User presses PageUp → `handle_conversation_input` scrolls within buffer
2. If near buffer edge: `buffer.request_load_older(20)` returns `Some((path, start, end))`
3. App spawns `parse_jsonl_range_async(path, start, end)` on `parse_tx` channel
4. Event loop receives result on `parse_rx` → calls `buffer.receive_loaded(result, width)`
5. Returns `(added_lines, evicted_lines)` → app adjusts `scroll_offset`
6. **Coalescing:** if user presses PageUp again while load is in flight, `request_load_older` returns `None` (load already pending). The pending load will extend the buffer enough for the new position. If the user overshoots the pending load, the next render cycle will trigger another load.

**Synchronous operations** (always immediate, no channel):
- `load_file()` — agent switch, builds index + parses tail
- `file_changed()` — watcher event, updates index + parses new lines if following

**Tool result merging:**
- Within a loaded range: `merge_tool_results()` runs on each batch, same as today
- At boundaries when appending: check if `entries.back()` is an unmerged `ToolCall` and first new entry is matching `ToolResult` → merge (reuses existing logic from `app.rs:462-479`)
- When loading a range, extend by +1 line at the trailing edge to capture potential ToolResults for ToolCalls at the boundary

**Eviction:**
- `receive_loaded()` returns `(added_lines, evicted_lines)` computed via `calculate_entry_lines()` for the affected entries
- The caller (App) adjusts `scroll_offset` accordingly:
  - Prepend (Older): `scroll_offset += added_lines` (content shifted down)
  - Evict from back: no scroll_offset change (content above viewport unchanged)
  - Append (Newer): no scroll_offset change
  - Evict from front: `scroll_offset -= evicted_lines`
  - Replace (jump): reset scroll_offset entirely

#### Phase 4: Integration with App (`src/app.rs`)

**Replace fields:**
- `conversation: VecDeque<DisplayEntry>` → `buffer: EntryBuffer`
- Remove `entries_truncated: usize` (buffer handles capacity internally)
- Remove `is_refreshing` flag (tail updates are synchronous)
- Keep `parse_errors: Vec<String>` (delegate to `buffer.parse_errors()`)

**Repurpose parse channel:**
- Keep `parse_tx`/`parse_rx` — now used for async scroll loads instead of initial file parse
- Keep `is_parsing` — now means "async scroll load in flight"
- Remove `parsing_path` — replaced by `buffer.pending_load`

**Update methods:**

`load_conversation_for_selected_agent()`:
- Call `buffer.load_file(&path)` (synchronous — index scan + parse tail)
- Start watcher on the path
- Reset `conversation_state` (scroll_offset = 0, follow_mode = true)

`refresh_conversation()` (watcher event):
- Call `buffer.file_changed(conversation_state.follow_mode)` (synchronous)
- If entries were added and follow_mode, rendering auto-scrolls (existing behavior)

`handle_parse_complete()`:
- Renamed/repurposed for async scroll loads
- Calls `buffer.receive_loaded(result, content_width)`
- Adjusts `scroll_offset` using returned `(added, evicted)` values

**Remove:**
- `apply_conversation_limit()` — buffer handles this
- `ParseMessage::Complete` path field — not needed, buffer tracks pending load

**Keep:** `discovery_rx/tx` channels for project/session discovery (unchanged).

#### Phase 5: Scrolling Changes (`src/input/handler.rs`, `src/ui/conversation.rs`)

**ConversationState** — unchanged:
```rust
pub struct ConversationState {
    pub scroll_offset: usize,    // rendered line offset within the buffer
    pub total_lines: usize,      // rendered lines in current buffer
    pub follow_mode: bool,
}
```

The `scroll_offset` and `total_lines` refer to rendered lines within the buffered entries. All scrolling math stays identical.

**handle_conversation_input changes:**

For `j`/`k` (single line scroll):
- Same as today, but after scrolling, check if near buffer edge
- If `scroll_offset < threshold` and `buffer.has_older()`:
  - `buffer.request_load_older(20)` → spawn async parse if Some
- If `scroll_offset > total_lines - threshold` and `buffer.has_newer()`:
  - `buffer.request_load_newer(20)` → spawn async parse if Some

For `g` (go to top):
- `buffer.request_jump_to_start()` → spawn async parse
- Set `follow_mode = false`, `scroll_offset = 0`
- Buffer will replace contents when load completes

For `G` (go to bottom):
- `buffer.request_jump_to_end()` → spawn async parse
- Set `follow_mode = true`
- Buffer will replace contents when load completes

For `Ctrl+d/u`, `PageUp/Down`:
- Same scroll_offset change as today
- Same buffer-edge check as j/k but with larger threshold

**Scrollbar:** Change from exact rendered-line position to approximate JSONL-line position:
```rust
let (win_start, win_end) = buffer.window_position();
let total = buffer.total_file_lines();
let scroll_fraction = scroll_offset as f64 / total_lines.max(1) as f64;
let approx_position = win_start as f64 + scroll_fraction * (win_end - win_start) as f64;
let scrollbar_state = ScrollbarState::default()
    .content_length(total)
    .position(approx_position as usize)
    .viewport_content_length(viewport_height);
```

**ConversationView changes:**
- Constructor takes `buffer.entries()` — `&VecDeque<DisplayEntry>`, same type as today
- `calculate_total_lines`, `render_entries` — unchanged, they operate on the buffer's entries
- Scrollbar rendering — use approximate position as above

#### Phase 6: Event Loop (`src/main.rs`)

Current branches remain, with modified semantics:

```
1. Keyboard events → handle_input()           (may trigger async load request)
2. Watcher events  → buffer.file_changed()    (synchronous tail update)
3. parse_rx        → buffer.receive_loaded()  (async scroll load results)
4. discovery_rx    → handle discovery          (unchanged)
5. Periodic refresh                            (unchanged)
```

Branch 2 simplifies (no async, just call `file_changed`). Branch 3 changes from "initial parse complete" to "scroll load complete".

#### Files to Modify

| File | Change |
|------|--------|
| `src/logs/index.rs` | **NEW** — LineIndex struct |
| `src/logs/buffer.rs` | **NEW** — EntryBuffer struct |
| `src/logs/parser.rs` | **ADD** `parse_jsonl_range`, `parse_jsonl_range_async` |
| `src/logs/mod.rs` | **ADD** exports for new modules |
| `src/app.rs` | **REPLACE** conversation/parse machinery with EntryBuffer |
| `src/ui/conversation.rs` | **UPDATE** scrollbar to use approximate position |
| `src/input/handler.rs` | **UPDATE** scroll handlers to trigger buffer loading at edges |
| `src/main.rs` | **UPDATE** event loop for new sync/async split |

#### Verification

1. `cargo check` — type checking
2. `cargo clippy` — linting
3. `cargo test` — existing tests + new tests for LineIndex, parse_jsonl_range, EntryBuffer
4. `cargo run` — manual testing:
   - Open a conversation, verify it displays correctly
   - Scroll up/down through history, verify entries load on demand
   - Press `g`/`G` to jump to top/bottom
   - Rapid PageUp — should stay responsive, no blocking
   - Watch a live conversation (tail mode), verify new entries appear
   - Switch agents, verify quick load
   - Check scrollbar position is reasonable
   - Resize terminal, verify rendering still works
5. `cargo build --release` — release build

## Acceptance Criteria

- Conversation displays correctly on open
- Scrolling up/down loads entries on demand (no full-file parse)
- Rapid PageUp stays responsive (async loading, no blocking)
- g/G jump to top/bottom work
- Tail mode: new entries appear live
- Agent switching is fast (index scan + parse tail only)
- Scrollbar shows approximate position
- All existing tests pass
- New tests for LineIndex, parse_jsonl_range, EntryBuffer

## Bug Fix: Text Wrapping Mismatch

### Problem

After initial implementation, users reported critical scrolling bugs:
1. Scroll momentum: holding arrow keys causes scrolling to continue after key release (queued events)
2. Can't scroll back to bottom after PageUp to top
3. Scrollbar disappearing
4. Blank/truncated text display

### Root Cause

Line count calculations were inconsistent between two locations:

- **buffer.rs** `wrap_text_line_count` (lines 509-523): Used simple character-width division without word wrapping
  ```rust
  count += (line_len + width - 1) / width.max(1);  // ceiling division
  ```

- **conversation.rs** `wrap_text` (lines 1228-1259): Used proper word-aware wrapping
  ```rust
  // Splits at word boundaries, different line count than character division
  ```

When `receive_loaded()` calculated `scroll_delta` using buffer.rs line counts, those counts differed from actual rendered lines in conversation.rs. This caused scroll_offset to point to wrong positions, leading to all reported symptoms.

### Solution

Created `src/text_utils.rs` as shared module with single source of truth for text wrapping:
- `wrap_text()`: Word-aware wrapping function
- `wrap_text_line_count()`: Calls `wrap_text().len()` for guaranteed consistency

Both `buffer.rs` and `conversation.rs` now import and use these shared functions, ensuring line count calculations match actual rendering.

### Files Changed

- `src/text_utils.rs` (new): Shared text wrapping utilities
- `src/lib.rs`: Added text_utils module
- `src/main.rs`: Added text_utils module  
- `src/logs/buffer.rs`: Import and use shared `wrap_text_line_count`, removed local version
- `src/ui/conversation.rs`: Import and use shared `wrap_text`, removed local version
