# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
cargo build                          # dev build
cargo build --release                # release build
cargo install --path . --root ~/.local  # install to ~/.local/bin
cargo check                          # fast validation
cargo test                           # all tests
cargo test <name>                    # single test by name
cargo test multiplexer               # tests in a module
cargo test -- --nocapture            # show println! output
cargo clippy --all-targets           # lint
cargo fmt                            # format
```

Logs go to `~/.cache/workmux/workmux.log`. Enable with `RUST_LOG=debug workmux <cmd>`.

## Architecture

### Layers (top to bottom)

**CLI** (`src/cli.rs`, `src/command/`) — clap-based argument parsing. Each subcommand has its own file under `src/command/`. The dashboard is `src/command/dashboard/`.

**Workflow** (`src/workflow/`) — Orchestrates user-facing operations. Workflows take a `WorkflowContext` and return typed results. Key workflows:
- `create` — git worktree + multiplexer window/session + pane setup + hooks
- `open` — reattach to existing worktree
- `remove` — teardown (kills window, deletes worktree dir, optionally deletes branch)
- `merge` — merge branch into target + cleanup
- `list` — enumerate git worktrees + general sessions from live pane scan
- `create_general_session` — non-git session (skips all git operations)

**Multiplexer** (`src/multiplexer/`) — Trait abstraction over tmux, WezTerm, Kitty. Backend is auto-detected from env vars (`$TMUX` → tmux, `$WEZTERM_PANE` → wezterm, `$KITTY_WINDOW_ID` → kitty) with `$WORKMUX_BACKEND` override. All window/session operations go through this trait; `MuxHandle` dispatches between window mode and session mode transparently.

**State Store** (`src/state/`) — Filesystem persistence at `~/.local/state/workmux/agents/{backend}__{instance}__{pane_id}.json`. Tracks agent status (working/waiting/done), working directory, PID, and command per pane. `load_reconciled_agents()` cross-references live pane state and cleans stale entries.

**Config** (`src/config/`) — Loads `.workmux.yaml` (project-level, walks up from CWD) then `~/.config/workmux/config.yaml` (user-level). Controls pane layout, hooks, file sync, agent, merge strategy, window prefix, and dashboard settings.

### Window Naming

Every managed window/session is named `{prefix}{handle}`. Default prefix is `"wm-"` (configurable, supports nerd font icons). Handle is the worktree directory basename for git sessions, or the arbitrary name passed to `workmux start` for general sessions. The prefix is how workmux identifies its own windows when scanning tmux.

### Git Worktree Sessions vs General Sessions

- **Git sessions** (`workmux add/create`): backed by `git worktree add`, branch-tracked, support merge/remove with git cleanup. `WorkflowContext::new()` requires a git repo.
- **General sessions** (`workmux start`): arbitrary directories, no git context. `WorkflowContext::new_general()` skips all git checks. Cleanup just kills the mux target.

The `list` workflow handles both: git worktrees from `git worktree list`, then live pane scan for `wm-*` windows not covered by any worktree.

### Agent Hook System

Agent status (working/waiting/done) is reported back to workmux via Claude Code hooks that call `workmux status <state>`. This writes to the state store. The dashboard reads from the state store to display live status. `workmux setup` installs these hooks into `~/.claude/settings.json`. Agent profiles in `src/multiplexer/agent.rs` define per-agent CLI behavior (prompt injection format, startup delays, etc.).

### Dashboard

TUI built with ratatui. On each tick: loads reconciled agents from state store, scans live panes for `wm-*` windows without state store entries (shows unconfigured/new sessions), fetches git status per worktree path, optionally fetches PR status from GitHub. Actions (`[c]` commit, `[d]` diff, `[x]` close, `[r]` remove) send keystrokes directly to the agent pane. Columns: `# | Project | Worktree | Git | (PR) | Status | Time | Title`.

## Future Considerations

### Dashboard: closed worktree visibility
`workmux close` kills the tmux window but preserves the branch/worktree. Closed worktrees disappear from the dashboard (which only shows live windows) and must be rediscovered via `workmux list` in the terminal, then reopened with `workmux open <branch>`. For `close` to be a fully useful dashboard action, the dashboard should show closed worktrees as dimmed rows with a reopen action `[o]`, merging `workflow::list()` results with the live-window scan. Out of scope for now.
