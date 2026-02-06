---
id: ct-y9t5
status: closed
deps: []
links: []
created: 2026-02-06T12:34:55Z
type: feature
priority: 2
assignee: Jeffery Utter
tags: [planned]
---
# Add super-follow flag

add a --super-follow (-s) cli argument. When in 'super follow' mode, we should automatically switch to the project/session/agent with the most recent activity, whenever we check for new activity

## Design

### Overview

Add a `--super-follow` (`-s`) CLI flag that enables automatic navigation to the most recently active project/session/agent whenever activity is detected. This builds on the existing follow mode (which auto-scrolls within a conversation) by adding auto-navigation across the hierarchy.

### Implementation Steps

#### 1. Add CLI Argument (main.rs)

In the `Args` struct, add:
```rust
/// Enable automatic switching to project/session/agent with most recent activity
#[arg(short = 's', long)]
super_follow: bool,
```

#### 2. Store in App State (app.rs)

Add field to `App` struct:
```rust
pub super_follow_enabled: bool,
```

Update `App::new()` signature to accept the flag and store it.

Update the call site in `main.rs` to pass `args.super_follow`.

#### 3. Implement Auto-Switch Logic (app.rs)

Add method `auto_switch_to_most_recent(&mut self)` that:

1. Checks if `super_follow_enabled` is true; returns early if not
2. Finds the project with the most recent `last_modified` timestamp
3. If different from current selection:
   - Calls `project_state.select(Some(0))` (projects are already sorted by recency)
   - Triggers `load_sessions_for_selected_project()`
4. After sessions load, select the first session (most recent due to sorting)
5. After agents load, select the first non-main agent with most recent activity, or main agent if no sub-agents
6. Load conversation for selected agent

**Key insight**: Projects, sessions, and agents are already sorted by `last_modified` descending in their respective discovery functions. So "most recent" is always index 0 (or index 1 for agents, since main agent is pinned at 0).

#### 4. Integrate with Event Loop (main.rs)

Call `app.auto_switch_to_most_recent()` in two places:

1. After `list_refresh_interval.tick()` completes the refresh cycle
2. After `app.watcher.next_event()` detects a file modification (optionally, since the periodic refresh will catch it)

#### 5. Handle Discovery Completion

The discovery system uses async channels (`discovery_rx`). After receiving `DiscoveryMessage::ProjectsComplete` or `DiscoveryMessage::SessionsComplete`, call the auto-switch logic to navigate to the newly-discovered most-recent item.

### Edge Cases

- **User manual navigation**: If user manually navigates (j/k keys), consider temporarily disabling super-follow for that pane until the next periodic refresh. This prevents fighting with user input.
- **Empty lists**: Guard against empty project/session/agent lists before selecting index 0.
- **Main agent priority**: When auto-switching agents, prefer sub-agents with activity over the main agent, unless main agent is the only one or has the most recent activity.

### Testing

Manual testing scenarios:
1. Start app with `-s` flag, verify it auto-switches to most recent session
2. Start a new Claude session in another terminal, verify app switches to it
3. Manual navigation should still work (may temporarily override super-follow)
4. Without `-s` flag, verify no auto-switching occurs

### Files Modified

- `src/main.rs`: Add CLI arg, pass to App, integrate with event loop
- `src/app.rs`: Add state field, implement `auto_switch_to_most_recent()` method

