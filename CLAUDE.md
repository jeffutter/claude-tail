# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**claude-tail** is a Rust TUI application for viewing and browsing Claude.ai conversation logs. It reads JSONL log files from `~/.claude/projects/` and presents them in an interactive three-pane interface (Projects → Sessions → Conversations).

## Build & Run Commands

```bash
cargo build              # Development build
cargo build --release    # Optimized build (uses LTO)
cargo run                # Run in development mode
cargo run --release      # Run optimized
cargo check              # Fast type checking without building
cargo clippy             # Linting
cargo fmt                # Format code
```

**Nix users**: The `flake.nix` provides a reproducible dev environment with Rust toolchain and rust-analyzer.

## Architecture

```
src/
├── main.rs         # Entry point, event loop, terminal setup
├── app.rs          # Application state (focus, projects, sessions, conversation)
├── input/
│   └── handler.rs  # Keyboard event handling (vim-style navigation)
├── logs/
│   ├── types.rs    # Data structures matching Claude's API format
│   ├── parser.rs   # JSONL → DisplayEntry conversion
│   ├── project.rs  # Project/session discovery from ~/.claude/projects/
│   └── watcher.rs  # File system monitoring with debouncing
└── ui/
    ├── styles.rs       # Theme and color definitions
    ├── layout.rs       # Dynamic pane layout (collapses unfocused panes)
    ├── conversation.rs # Main conversation view rendering
    ├── project_list.rs # Projects list widget
    └── session_list.rs # Sessions list widget
```

**Data flow**: File System → Watcher → App Event → Parser → DisplayEntry → UI Widgets → Terminal

**Key dependencies**: `ratatui` (TUI rendering), `crossterm` (terminal control), `tokio` (async runtime), `notify` (file watching), `serde_json` (parsing)

## Key Concepts

- **Three-pane navigation**: Projects, Sessions, Conversation. Tab cycles focus.
- **DisplayEntry**: Normalized representation of log content (user messages, assistant text, tool calls, thinking blocks, etc.)
- **Follow mode**: Auto-scroll to latest content when enabled (toggle with `f`)
- **Incremental parsing**: Parser tracks file position to efficiently handle streaming updates
- **Path encoding**: Projects use encoded paths (dashes replace slashes) with fallback to `sessions-index.json` for originals
