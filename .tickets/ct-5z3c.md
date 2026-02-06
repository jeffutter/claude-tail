---
id: ct-5z3c
status: open
deps: []
links: []
created: 2026-02-06T12:21:08Z
type: task
priority: 1
assignee: Jeffery Utter
parent: ct-mep4
tags: [planned]
---
# Remove per-frame make_contiguous() call

main.rs line 227 calls app.conversation.make_contiguous() every frame, potentially causing O(n) reallocation of the entire VecDeque (up to 10k entries). Either remove this call or restructure to avoid per-frame contiguity requirements.

## Design

1. Investigate why make_contiguous() is called (likely for slice access in render)
2. Option A: Use VecDeque iterator instead of slice in render_entries()
3. Option B: Only call make_contiguous() when conversation changes, not every frame
4. Option C: Switch from VecDeque to Vec if FIFO behavior isn't needed

