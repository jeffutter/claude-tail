---
id: ct-ywkl
status: open
deps: []
links: []
created: 2026-02-06T04:57:28Z
type: task
priority: 2
assignee: Jeffery Utter
parent: ct-a9d4
---
# Optimize merge_tool_results to avoid cloning all entries

merge_tool_results() in parser.rs:445-501 clones every entry when building the merged result vector. For large conversations (10k+ entries), this adds 50-150ms of CPU time. Refactor to use in-place mutation or consume the input vector instead of cloning, only allocating new entries when actually merging tool results together.

