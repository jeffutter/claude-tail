---
id: ct-xwz9
status: closed
deps: []
links: []
created: 2026-02-14T23:38:37Z
type: chore
priority: 3
assignee: Jeffery Utter
tags: [planned]
---
# Add cargo bench and cargo nextest to Claude settings permissions

Update .claude/settings.json to allow cargo bench, cargo test, and cargo nextest commands in the permitted Bash commands list.

## Change
Add to the allow list in .claude/settings.json:
  "Bash(cargo bench:*)",
  "Bash(cargo test:*)",
  "Bash(cargo nextest:*)"

Currently only cargo build, cargo clippy, cargo fmt are permitted.

## Files
- .claude/settings.json

## Design

Add three entries to the `permissions.allow` array in `.claude/settings.json`, after the existing `cargo fmt:*` entry:

```json
"Bash(cargo bench:*)",
"Bash(cargo nextest:*)",
"Bash(cargo test:*)"
```

Alphabetical order within the cargo group. No other changes needed.

## Notes
Source commit: 0bb27b7 on ai-slop-refactor. Tiny change but important for agent workflows involving benchmarks and testing. Independent of other tickets.

