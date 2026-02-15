use std::io::Write as _;

use claude_tail::logs::parser::{merge_tool_results, parse_jsonl_file, parse_jsonl_from_position};
use tempfile::NamedTempFile;

// ── Data generators ───────────────────────────────────────────────────────────

/// Write `n` JSONL lines to a temp file using a rotating mix of entry types:
/// ~40% user messages, ~40% assistant messages, ~20% assistant with ToolUse blocks.
fn generate_jsonl_file(n: usize) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    for i in 0..n {
        let line = match i % 5 {
            0 | 1 => format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"message {i}\"}}}}",
            ),
            2 | 3 => format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"response {i}\"}}}}",
            ),
            _ => format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"id\":\"tool-{i}\",\"name\":\"Bash\",\"input\":{{\"command\":\"echo {i}\"}}}}]}}}}",
            ),
        };
        writeln!(file, "{line}").expect("write");
    }
    file.flush().expect("flush");
    file
}

/// Write `n` JSONL lines with malformed lines injected at `error_rate` (0–100 integer percent).
fn generate_jsonl_file_with_errors(n: usize, error_rate_pct: u8) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    for i in 0..n {
        let is_error = error_rate_pct > 0 && (i % (100 / error_rate_pct as usize)) == 0;
        let line = if is_error {
            format!("{{bad json line {i}")
        } else {
            format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"message {i}\"}}}}",
            )
        };
        writeln!(file, "{line}").expect("write");
    }
    file.flush().expect("flush");
    file
}

/// Write `n` entries where ~50% are ToolUse followed by matching ToolResult entries.
/// Each pair produces a ToolCall + ToolResult that can be merged.
fn generate_jsonl_file_with_tools(n: usize) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    let mut i = 0;
    while i < n {
        if i + 1 < n {
            // ToolUse in an assistant message
            let tool_line = format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"id\":\"tool-{i}\",\"name\":\"Bash\",\"input\":{{\"command\":\"echo {i}\"}}}}]}}}}",
            );
            // Matching ToolResult in a user message
            let result_line = format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"tool-{i}\",\"content\":\"output {i}\"}}]}}}}",
            );
            writeln!(file, "{tool_line}").expect("write");
            writeln!(file, "{result_line}").expect("write");
            i += 2;
        } else {
            let line = format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"message {i}\"}}}}",
            );
            writeln!(file, "{line}").expect("write");
            i += 1;
        }
    }
    file.flush().expect("flush");
    file
}

/// Find the byte offset of the nearest newline at or after `approx_pos`.
/// Returns a position at the start of a valid line.
/// Special case: `approx_pos = 0` returns 0 so the parse starts at the true beginning.
fn find_line_boundary(file: &NamedTempFile, approx_pos: u64) -> u64 {
    if approx_pos == 0 {
        return 0;
    }
    let content = std::fs::read(file.path()).expect("read file");
    let total = content.len() as u64;
    if approx_pos >= total {
        return total;
    }
    let start = approx_pos as usize;
    if let Some(rel) = content[start..].iter().position(|&b| b == b'\n') {
        (start + rel + 1) as u64
    } else {
        total
    }
}

// ── Benchmark group 1: Full parse ────────────────────────────────────────────

#[divan::bench(args = [100, 1000, 10000])]
fn full_parse(bencher: divan::Bencher, n: usize) {
    let file = generate_jsonl_file(n);
    bencher.bench(|| parse_jsonl_file(file.path()).unwrap());
}

// ── Benchmark group 2: Incremental parse ─────────────────────────────────────

/// Benchmark resuming from various percentages into a 1000-entry file.
#[divan::bench(args = [0usize, 25, 50, 75, 100])]
fn incremental_parse(bencher: divan::Bencher, pct: usize) {
    let file = generate_jsonl_file(1000);
    let total_bytes = std::fs::metadata(file.path()).unwrap().len();
    let approx = total_bytes * pct as u64 / 100;
    let position = find_line_boundary(&file, approx);
    bencher.bench(|| parse_jsonl_from_position(file.path(), position).unwrap());
}

// ── Benchmark group 3: Error recovery ────────────────────────────────────────

/// Integer percent error rates: 0, 1, 5, 10.
#[divan::bench(args = [0u8, 1, 5, 10])]
fn error_recovery(bencher: divan::Bencher, error_rate_pct: u8) {
    let file = generate_jsonl_file_with_errors(1000, error_rate_pct);
    bencher.bench(|| parse_jsonl_file(file.path()).unwrap());
}

// ── Benchmark group 4: Tool merge ────────────────────────────────────────────

#[divan::bench(args = [100, 500, 1000])]
fn tool_merge(bencher: divan::Bencher, n: usize) {
    let file = generate_jsonl_file_with_tools(n);
    let parsed = parse_jsonl_file(file.path()).unwrap();
    // Use with_inputs so the Vec clone happens in divan's setup phase, outside the timed region.
    bencher
        .with_inputs(|| parsed.entries.clone())
        .bench_values(merge_tool_results);
}

fn main() {
    divan::main();
}
