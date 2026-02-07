use claude_tail::logs::parser::{merge_tool_results, parse_jsonl_file, parse_jsonl_from_position};

fn main() {
    divan::main();
}

mod data_gen {
    //! Test data generation utilities

    use std::io::Write;
    use tempfile::NamedTempFile;

    pub fn user_entry(text: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#,
            text
        )
    }

    pub fn assistant_entry(text: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":"{}"}}}}"#,
            text
        )
    }

    pub fn tool_call_entry(name: &str, id: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"{}","name":"{}","input":{{}}}}]}}}}"#,
            id, name
        )
    }

    pub fn tool_result_entry(tool_use_id: &str, content: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{}","content":"{}"}}]}}}}"#,
            tool_use_id, content
        )
    }

    pub fn thinking_entry(text: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"{}"}}]}}}}"#,
            text
        )
    }

    pub fn malformed_entry() -> String {
        r#"{"type":"user","message":{"not valid json"#.to_string()
    }

    /// Generate a JSONL file with a mix of entry types
    pub fn generate_mixed_file(num_entries: usize) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        for i in 0..num_entries {
            let entry = match i % 5 {
                0 => user_entry(&format!("User message {}", i)),
                1 => assistant_entry(&format!("Assistant response {}", i)),
                2 => tool_call_entry("grep", &format!("tool-{}", i)),
                3 => tool_result_entry(&format!("tool-{}", i - 1), "Result"),
                _ => thinking_entry(&format!("Thinking about {}", i)),
            };
            writeln!(file, "{}", entry).unwrap();
        }
        file.flush().unwrap();
        file
    }

    /// Generate a file with some malformed entries
    pub fn generate_file_with_errors(num_entries: usize, error_rate: f32) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        for i in 0..num_entries {
            let entry = if (i as f32 / num_entries as f32) < error_rate && i % 10 == 5 {
                malformed_entry()
            } else {
                user_entry(&format!("Message {}", i))
            };
            writeln!(file, "{}", entry).unwrap();
        }
        file.flush().unwrap();
        file
    }
}

// Full file parsing benchmarks
mod full_parse {
    use super::*;

    #[divan::bench(args = [100, 1000, 10000])]
    fn parse_mixed_entries(bencher: divan::Bencher, count: usize) {
        let file = data_gen::generate_mixed_file(count);
        bencher.bench_local(|| parse_jsonl_file(file.path()).unwrap());
    }

    #[divan::bench]
    fn parse_small_file() {
        let file = data_gen::generate_mixed_file(100);
        divan::black_box(parse_jsonl_file(file.path()).unwrap());
    }

    #[divan::bench]
    fn parse_medium_file() {
        let file = data_gen::generate_mixed_file(1000);
        divan::black_box(parse_jsonl_file(file.path()).unwrap());
    }

    #[divan::bench]
    fn parse_large_file() {
        let file = data_gen::generate_mixed_file(10000);
        divan::black_box(parse_jsonl_file(file.path()).unwrap());
    }
}

// Incremental parsing benchmarks
mod incremental {
    use super::*;

    #[divan::bench(args = [1000, 5000, 10000])]
    fn resume_from_middle(bencher: divan::Bencher, total: usize) {
        let file = data_gen::generate_mixed_file(total);
        // Parse first half to get position
        let halfway = {
            let result = parse_jsonl_file(file.path()).unwrap();
            result.bytes_read / 2
        };

        bencher.bench_local(|| parse_jsonl_from_position(file.path(), halfway).unwrap());
    }

    #[divan::bench]
    fn incremental_append_simulation() {
        // Simulate reading an appended file multiple times
        let file = data_gen::generate_mixed_file(1000);
        let mut position = 0u64;

        for _ in 0..10 {
            let result = parse_jsonl_from_position(file.path(), position).unwrap();
            position = result.bytes_read;
            divan::black_box(result);
        }
    }
}

// Error recovery benchmarks
mod error_recovery {
    use super::*;

    #[divan::bench(args = [0.0, 0.01, 0.05, 0.10])]
    fn parse_with_error_rate(bencher: divan::Bencher, error_rate: f32) {
        let file = data_gen::generate_file_with_errors(1000, error_rate);
        bencher.bench_local(|| parse_jsonl_file(file.path()).unwrap());
    }
}

// Tool result merging benchmarks
mod merge {
    use super::*;

    #[divan::bench(args = [100, 500, 1000])]
    fn merge_tool_results_bench(bencher: divan::Bencher, count: usize) {
        let file = data_gen::generate_mixed_file(count);
        let result = parse_jsonl_file(file.path()).unwrap();

        bencher.bench_local(|| merge_tool_results(result.entries.clone()));
    }
}
