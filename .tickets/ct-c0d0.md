---
id: ct-c0d0
status: open
deps: [ct-kkil, ct-5vmd]
links: []
created: 2026-02-14T23:38:12Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [needs-plan]
---
# Add comprehensive unit tests for JSONL parser

Add inline test module to src/logs/parser.rs covering byte position tracking, incremental parsing, error recovery, and edge cases.

## Scope (29 tests on ai-slop-refactor, across 5 categories)
1. Byte position tracking (6 tests): single entry, multiple entries, empty lines, trailing newline, no trailing newline
2. Resume from position (5 tests): from start, from middle, from end, incremental append simulation
3. Partial/incomplete lines at EOF (5 tests): truncated JSON, partial line with no newline, mid-stream append
4. Error recovery (7 tests): malformed JSON on one line, multiple bad lines, bad line followed by good, mixed
5. Edge cases (5 tests): UTF-8 multi-byte characters, CRLF line endings, very long lines, empty file, whitespace-only

## Key behavior to verify
- parse_jsonl_from_position() resumes correctly without re-reading already-consumed bytes
- Incomplete JSON at EOF is NOT treated as an error (just not consumed)
- CRLF (\r\n) endings are handled correctly in byte position math (was buggy before StreamDeserializer)
- Error lines are skipped and reported but don't abort parsing

Example:
  #[test]
  fn test_resume_from_position() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("test.jsonl");
      // write first entry, parse it, get bytes_read
      // append second entry, parse from bytes_read
      // verify only second entry returned
  }

## Files
- src/logs/parser.rs (inline #[cfg(test)] module)
- Cargo.toml (add tempfile dev-dependency)

## Notes
Source commits: d510a39 (initial tests) + fixes in 7842d26 (CRLF test correction) on ai-slop-refactor. Depends on ct-kkil (tagged enum) and ct-5vmd (StreamDeserializer, for correct CRLF behavior). Re-planning required.

