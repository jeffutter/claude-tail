---
id: ct-5vmd
status: open
deps: [ct-kkil]
links: []
created: 2026-02-14T23:37:52Z
type: task
priority: 2
assignee: Jeffery Utter
tags: [needs-plan]
---
# Refactor JSONL parser to use StreamDeserializer

Replace the current line-based JSONL parsing approach with serde_json::StreamDeserializer. Primary motivation is a CRLF bug fix; secondary benefit is cleaner code and modest performance improvement.

## Problem
Current line-based parser in src/logs/parser.rs iterates lines via content.lines(), manually tracking byte offsets. This has a bug: CRLF line endings (\r\n) cause silent position undercounting because .lines() strips \r but the manual offset math doesn't account for it.

## Target approach
Use serde_json::Deserializer::from_str() with into_iter::<LogEntry>() (StreamDeserializer). Key improvements:
  - Automatic byte position tracking via stream.byte_offset()
  - Cleaner EOF detection via error.classify() returning Category::Eof vs Category::Syntax
  - Eliminates manual newline/CRLF accounting
  - ~3-5% performance improvement

Core parsing loop changes from line iteration to:
  while current_pos < content.len() {
      let slice = &content[current_pos..];
      let deserializer = serde_json::Deserializer::from_str(slice);
      let mut stream = deserializer.into_iter::<LogEntry>();
      match stream.next() {
          Some(Ok(entry)) => {
              entries.extend(convert_log_entry(&entry));
              current_pos += stream.byte_offset();
              // skip trailing whitespace
          }
          Some(Err(e)) if e.classify() == Category::Eof => break,
          Some(Err(e)) => { errors.push(...); advance past error... }
          None => break,
      }
  }

Error messages change from 'Line N: ...' to 'Parse error at byte N: ...'.

## Files
- src/logs/parser.rs

## Notes
Source commit: 7842d26 on ai-slop-refactor. Depends on LogEntry tagged enum (ct-kkil) being implemented first as the pattern matching in convert_log_entry will differ. Re-planning required.

