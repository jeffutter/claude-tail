# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**claude-tail** is a Rust TUI application for viewing and browsing Claude.ai conversation logs. It reads JSONL log files from `~/.claude/projects/` and presents them in an interactive four-pane interface (Projects → Sessions → Agents → Conversation).

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
├── app.rs          # Application state (focus, projects, sessions, agents, conversation)
├── input/
│   └── handler.rs  # Keyboard event handling (vim-style navigation)
├── logs/
│   ├── types.rs    # Data structures matching Claude's API format
│   ├── parser.rs   # JSONL → DisplayEntry conversion
│   ├── project.rs  # Project/session/agent discovery from ~/.claude/projects/
│   └── watcher.rs  # File system monitoring with debouncing
├── themes/
│   ├── mod.rs      # Theme loading (bundled + custom from ~/.config/claude-tail/themes/)
│   └── presets/    # Built-in Base16 themes (tokyonight-storm, catppuccin-mocha, etc.)
└── ui/
    ├── styles.rs       # Theme application and color definitions
    ├── layout.rs       # Dynamic pane layout (collapses unfocused panes)
    ├── conversation.rs # Main conversation view rendering
    ├── project_list.rs # Projects list widget
    ├── session_list.rs # Sessions list widget
    └── agent_list.rs   # Agents list widget (main agent + sub-agents)
```

**Data flow**: File System → Watcher → App Event → Parser → DisplayEntry → UI Widgets → Terminal

**Key dependencies**: `ratatui` (TUI rendering), `crossterm` (terminal control), `tokio` (async runtime), `notify` (file watching), `serde_json`/`serde_yaml` (parsing)

## Key Concepts

- **Four-pane navigation**: Projects, Sessions, Agents, Conversation. Tab cycles focus. Unfocused panes collapse to single-letter indicators.
- **Agents**: Each session has a main agent plus optional sub-agents (stored in `{session_id}/subagents/`).
- **DisplayEntry**: Normalized log content—user messages, assistant text, tool calls, thinking blocks, hook events, agent spawns.
- **Themes**: Base16 color schemes. Six bundled themes; custom themes load from `~/.config/claude-tail/themes/`.
- **Follow mode**: Auto-scroll to latest content (toggle with `f`).
- **Incremental parsing**: Parser tracks file position to handle streaming updates efficiently.
- **Path encoding**: Projects use encoded paths (dashes replace slashes) with fallback to `sessions-index.json` for originals.
