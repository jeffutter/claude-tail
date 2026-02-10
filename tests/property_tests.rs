use claude_tail::logs::parser::{merge_tool_results, parse_jsonl_file};
use claude_tail::logs::{DisplayEntry, EntryBuffer, parse_jsonl_range};
use claude_tail::ui::conversation::ConversationView;
use claude_tail::ui::{ConversationState, Theme};
use proptest::prelude::*;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

// Fixed test parameters
const VIEWPORT_HEIGHT: usize = 50;
const CONTENT_WIDTH: usize = 80;
const SHOW_THINKING: bool = false;
const EXPAND_TOOLS: bool = false;
const BUFFER_CAPACITY: usize = 100;

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// Generate random ASCII text of the given length range (no newlines, no control chars)
fn arb_text(min: usize, max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::char::range(' ', '~'), min..=max)
        .prop_map(|chars| chars.into_iter().collect::<String>())
}

/// Generate multi-paragraph text (with embedded newlines for tall entries)
fn arb_multiline_text(min_lines: usize, max_lines: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(arb_text(10, 120), min_lines..=max_lines)
        .prop_map(|lines| lines.join("\n"))
}

/// Generate a user message JSONL line
fn arb_user_message() -> impl Strategy<Value = String> {
    arb_text(1, 200).prop_map(|text| {
        serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": text
            }
        })
        .to_string()
    })
}

/// Generate an assistant text JSONL line (sometimes multi-paragraph)
fn arb_assistant_text() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => arb_text(1, 500).prop_map(|text| {
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": text
                }
            })
            .to_string()
        }),
        1 => arb_multiline_text(2, 8).prop_map(|text| {
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": text
                }
            })
            .to_string()
        }),
    ]
}

/// Generate a tool_use ID
fn arb_tool_id() -> impl Strategy<Value = String> {
    proptest::string::string_regex("toolu_[a-zA-Z0-9]{10}").unwrap()
}

/// Generate a tool_use + tool_result pair as two JSONL lines
fn arb_tool_call_pair() -> impl Strategy<Value = Vec<String>> {
    (
        prop_oneof![
            Just("Bash"),
            Just("Read"),
            Just("Edit"),
            Just("Grep"),
            Just("Glob"),
            Just("Write"),
        ],
        arb_tool_id(),
        arb_text(5, 100),  // tool input content
        arb_text(5, 200),  // tool result content
        any::<bool>(),      // is_error
    )
        .prop_map(|(tool_name, id, input_text, result_text, is_error)| {
            let input = match tool_name {
                "Bash" => serde_json::json!({ "command": input_text }),
                "Read" => serde_json::json!({ "file_path": format!("/tmp/{}", input_text.chars().take(20).collect::<String>()) }),
                "Edit" => serde_json::json!({
                    "file_path": "/tmp/test.rs",
                    "old_string": input_text,
                    "new_string": "replaced"
                }),
                "Grep" => serde_json::json!({ "pattern": input_text.chars().take(30).collect::<String>() }),
                "Glob" => serde_json::json!({ "pattern": "**/*.rs" }),
                "Write" => serde_json::json!({
                    "file_path": "/tmp/test.rs",
                    "content": input_text
                }),
                _ => serde_json::json!({ "input": input_text }),
            };

            let assistant_line = serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": id,
                        "name": tool_name,
                        "input": input
                    }]
                }
            })
            .to_string();

            let user_result_line = serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": result_text,
                        "is_error": is_error
                    }]
                }
            })
            .to_string();

            vec![assistant_line, user_result_line]
        })
}

/// Generate a thinking block JSONL line
fn arb_thinking() -> impl Strategy<Value = String> {
    arb_text(10, 300).prop_map(|text| {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "thinking",
                    "thinking": text,
                    "signature": "sig_test"
                }]
            }
        })
        .to_string()
    })
}

/// Generate a hook event JSONL line
fn arb_hook_event() -> impl Strategy<Value = String> {
    (
        prop_oneof![
            Just("PreToolUse"),
            Just("PostToolUse"),
            Just("Notification"),
        ],
        proptest::option::of(Just("PostToolUse:Read".to_string())),
        proptest::option::of(arb_text(5, 50)),
    )
        .prop_map(|(event, hook_name, command)| {
            let mut data = serde_json::json!({ "hookEvent": event });
            if let Some(name) = hook_name {
                data["hookName"] = serde_json::json!(name);
            }
            if let Some(cmd) = command {
                data["command"] = serde_json::json!(cmd);
            }
            serde_json::json!({
                "type": "progress",
                "data": data
            })
            .to_string()
        })
}

/// Generate an agent spawn JSONL line
fn arb_agent_spawn() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("Bash"), Just("Explore"), Just("general-purpose")],
        arb_text(5, 50),
    )
        .prop_map(|(agent_type, description)| {
            serde_json::json!({
                "type": "progress",
                "data": {
                    "agentType": agent_type,
                    "description": description
                }
            })
            .to_string()
        })
}

/// Generate a Vec of JSONL lines forming a valid conversation
fn arb_jsonl_entries(max_entries: usize) -> impl Strategy<Value = Vec<String>> {
    proptest::collection::vec(
        prop_oneof![
            4 => arb_user_message().prop_map(|s| vec![s]),
            4 => arb_assistant_text().prop_map(|s| vec![s]),
            2 => arb_tool_call_pair(),
            1 => arb_thinking().prop_map(|s| vec![s]),
            1 => arb_hook_event().prop_map(|s| vec![s]),
            1 => arb_agent_spawn().prop_map(|s| vec![s]),
        ],
        1..=max_entries,
    )
    .prop_map(|groups| groups.into_iter().flatten().collect())
}

// ---------------------------------------------------------------------------
// Scroll operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum ScrollOp {
    LineDown,
    LineUp,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    JumpToStart,
    JumpToEnd,
}

fn arb_scroll_ops(max_ops: usize) -> impl Strategy<Value = Vec<ScrollOp>> {
    proptest::collection::vec(
        prop_oneof![
            4 => Just(ScrollOp::LineDown),
            4 => Just(ScrollOp::LineUp),
            2 => Just(ScrollOp::HalfPageDown),
            2 => Just(ScrollOp::HalfPageUp),
            1 => Just(ScrollOp::PageDown),
            1 => Just(ScrollOp::PageUp),
            1 => Just(ScrollOp::JumpToStart),
            1 => Just(ScrollOp::JumpToEnd),
        ],
        0..=max_ops,
    )
}

// ---------------------------------------------------------------------------
// Reference model
// ---------------------------------------------------------------------------

struct ReferenceModel {
    entries: Vec<DisplayEntry>,
    scroll_offset: usize,
    total_lines: usize,
    viewport_height: usize,
}

impl ReferenceModel {
    fn new(entries: Vec<DisplayEntry>, viewport_height: usize, content_width: usize) -> Self {
        let deque: VecDeque<DisplayEntry> = entries.iter().cloned().collect();
        let total_lines = compute_total_lines(&deque, content_width);
        // Start at bottom (follow mode)
        let scroll_offset = total_lines.saturating_sub(viewport_height);
        Self {
            entries,
            scroll_offset,
            total_lines,
            viewport_height,
        }
    }

    fn apply(&mut self, op: ScrollOp) {
        let max_scroll = self.total_lines.saturating_sub(self.viewport_height);
        match op {
            ScrollOp::LineDown => {
                self.scroll_offset = (self.scroll_offset + 1).min(max_scroll);
            }
            ScrollOp::LineUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            ScrollOp::HalfPageDown => {
                self.scroll_offset =
                    (self.scroll_offset + self.viewport_height / 2).min(max_scroll);
            }
            ScrollOp::HalfPageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_height / 2);
            }
            ScrollOp::PageDown => {
                self.scroll_offset = (self.scroll_offset + self.viewport_height).min(max_scroll);
            }
            ScrollOp::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_height);
            }
            ScrollOp::JumpToStart => {
                self.scroll_offset = 0;
            }
            ScrollOp::JumpToEnd => {
                self.scroll_offset = max_scroll;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Viewport extraction
// ---------------------------------------------------------------------------

/// Compute total rendered lines for a set of entries using ConversationView's method.
fn compute_total_lines(entries: &VecDeque<DisplayEntry>, content_width: usize) -> usize {
    let theme = Theme::default();
    let view = ConversationView::new(
        entries,
        false,
        &theme,
        SHOW_THINKING,
        EXPAND_TOOLS,
        false,
        0,
        (0, 0),
    );
    view.calculate_total_lines(content_width + 4)
}

/// Extract plain text from rendered lines for comparison.
/// Uses the same rendering pipeline as the real app.
fn extract_viewport_text(
    entries: &VecDeque<DisplayEntry>,
    scroll_offset: usize,
    viewport_height: usize,
    render_width: usize, // This is the "padded width" passed to render_entries
) -> Vec<String> {
    let theme = Theme::default();
    let view = ConversationView::new(
        entries,
        false,
        &theme,
        SHOW_THINKING,
        EXPAND_TOOLS,
        false,
        0,
        (0, 0),
    );

    let (lines, render_offset) = view.render_entries(render_width, scroll_offset, viewport_height);

    // Slice the visible portion
    let skip = scroll_offset.saturating_sub(render_offset);
    lines
        .into_iter()
        .skip(skip)
        .take(viewport_height)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// SUT driver
// ---------------------------------------------------------------------------

/// Apply a scroll operation to the SUT (EntryBuffer + ConversationState),
/// then loop loading until the buffer covers the viewport.
fn apply_to_sut(
    buffer: &mut EntryBuffer,
    state: &mut ConversationState,
    op: ScrollOp,
    viewport_height: usize,
    content_width: usize,
) {
    // Calculate total_lines before the operation using the same method as reference model
    state.total_lines = compute_total_lines(buffer.entries(), content_width);

    // Apply the scroll operation
    match op {
        ScrollOp::LineDown => {
            state.scroll_down(1, viewport_height);
        }
        ScrollOp::LineUp => {
            state.scroll_up(1);
        }
        ScrollOp::HalfPageDown => {
            state.scroll_down(viewport_height / 2, viewport_height);
        }
        ScrollOp::HalfPageUp => {
            state.scroll_up(viewport_height / 2);
        }
        ScrollOp::PageDown => {
            state.scroll_down(viewport_height, viewport_height);
        }
        ScrollOp::PageUp => {
            state.scroll_up(viewport_height);
        }
        ScrollOp::JumpToStart => {
            if let Some((path, start, end)) = buffer.request_jump_to_start() {
                let result = parse_jsonl_range(&path, start, end);
                buffer.receive_loaded(result, content_width, SHOW_THINKING, EXPAND_TOOLS);
            }
            state.scroll_to_top();
            state.follow_mode = false;
        }
        ScrollOp::JumpToEnd => {
            if let Some((path, start, end)) = buffer.request_jump_to_end() {
                let result = parse_jsonl_range(&path, start, end);
                buffer.receive_loaded(result, content_width, SHOW_THINKING, EXPAND_TOOLS);
            }
            // Recalculate total_lines after loading
            state.total_lines = compute_total_lines(buffer.entries(), content_width);
            state.scroll_to_bottom(viewport_height);
            state.follow_mode = true;
        }
    }

    // Loading loop: keep loading until buffer covers what we need
    settle_buffer(buffer, state, viewport_height, content_width);
}

/// Load entries until the buffer covers the current viewport position.
fn settle_buffer(
    buffer: &mut EntryBuffer,
    state: &mut ConversationState,
    viewport_height: usize,
    content_width: usize,
) {
    let threshold = viewport_height / 2;
    let max_iterations = 50;

    for _ in 0..max_iterations {
        // Recalculate total_lines
        state.total_lines = compute_total_lines(buffer.entries(), content_width);

        let mut loaded = false;

        // Near top - load older
        if state.scroll_offset < threshold && buffer.has_older() {
            buffer.clear_rate_limit();
            if let Some((path, start, end)) = buffer.request_load_older(40) {
                let result = parse_jsonl_range(&path, start, end);
                let delta =
                    buffer.receive_loaded(result, content_width, SHOW_THINKING, EXPAND_TOOLS);
                if delta != 0 {
                    state.scroll_offset = (state.scroll_offset as isize + delta).max(0) as usize;
                }
                loaded = true;
            }
        }

        // Near bottom - load newer
        let near_bottom = state.scroll_offset
            > state
                .total_lines
                .saturating_sub(viewport_height + threshold);
        if near_bottom && buffer.has_newer() {
            buffer.clear_rate_limit();
            if let Some((path, start, end)) = buffer.request_load_newer(40) {
                let result = parse_jsonl_range(&path, start, end);
                let delta =
                    buffer.receive_loaded(result, content_width, SHOW_THINKING, EXPAND_TOOLS);
                if delta != 0 {
                    state.scroll_offset = (state.scroll_offset as isize + delta).max(0) as usize;
                }
                loaded = true;
            }
        }

        if !loaded {
            break;
        }
    }

    // Final total_lines update
    state.total_lines = compute_total_lines(buffer.entries(), content_width);

    // Clamp scroll offset
    let max_scroll = state.total_lines.saturating_sub(viewport_height);
    state.scroll_offset = state.scroll_offset.min(max_scroll);
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Write JSONL lines to a temp file, return the directory and path
fn write_jsonl_file(lines: &[String]) -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("conversation.jsonl");
    let mut file = File::create(&path).unwrap();
    for line in lines {
        writeln!(file, "{}", line).unwrap();
    }
    file.flush().unwrap();
    drop(file);
    (temp_dir, path)
}

// ---------------------------------------------------------------------------
// Property test
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_shrink_iters: 2000,
        .. ProptestConfig::default()
    })]

    #[test]
    fn scroll_model_equivalence(
        jsonl_lines in arb_jsonl_entries(200),
        ops in arb_scroll_ops(200),
    ) {
        // Skip trivially empty files
        if jsonl_lines.is_empty() {
            return Ok(());
        }

        // Write JSONL file
        let (_temp_dir, path) = write_jsonl_file(&jsonl_lines);

        // Parse entire file -> reference model
        let full_parse = parse_jsonl_file(&path).unwrap();
        let all_entries = merge_tool_results(full_parse.entries);

        if all_entries.is_empty() {
            return Ok(());
        }

        let mut ref_model = ReferenceModel::new(all_entries, VIEWPORT_HEIGHT, CONTENT_WIDTH);

        // Load file with EntryBuffer -> SUT
        let mut buffer = EntryBuffer::new(BUFFER_CAPACITY);
        buffer.load_file(&path).unwrap();
        let mut state = ConversationState::new();

        // Both start at the bottom (follow mode).
        // Initialize SUT total_lines.
        state.total_lines = compute_total_lines(buffer.entries(), CONTENT_WIDTH);
        state.scroll_offset = state.total_lines.saturating_sub(VIEWPORT_HEIGHT);
        state.follow_mode = false;

        // Settle the SUT in case its initial load didn't cover the viewport
        settle_buffer(&mut buffer, &mut state, VIEWPORT_HEIGHT, CONTENT_WIDTH);

        // The render_width is what gets passed to render_entries. In the real app,
        // this is the padded inner width. For our tests, we use CONTENT_WIDTH + 4
        // so that when render_entries does width.saturating_sub(4) it yields CONTENT_WIDTH.
        let render_width = CONTENT_WIDTH + 4;

        for (i, &op) in ops.iter().enumerate() {
            ref_model.apply(op);
            apply_to_sut(&mut buffer, &mut state, op, VIEWPORT_HEIGHT, CONTENT_WIDTH);

            // Extract viewports
            let ref_entries: VecDeque<DisplayEntry> =
                ref_model.entries.iter().cloned().collect();
            let ref_viewport = extract_viewport_text(
                &ref_entries,
                ref_model.scroll_offset,
                VIEWPORT_HEIGHT,
                render_width,
            );
            let sut_viewport = extract_viewport_text(
                buffer.entries(),
                state.scroll_offset,
                VIEWPORT_HEIGHT,
                render_width,
            );

            prop_assert_eq!(
                &ref_viewport,
                &sut_viewport,
                "Viewports diverged after op #{} {:?} (ref_offset={}, sut_offset={}, sut_window={:?}, sut_entries={}, ref_entries={})",
                i,
                op,
                ref_model.scroll_offset,
                state.scroll_offset,
                buffer.window_position(),
                buffer.entries().len(),
                ref_model.entries.len(),
            );
        }
    }
}
