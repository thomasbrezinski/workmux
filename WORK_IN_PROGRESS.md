# workmux general-sessions — Work in Progress

## Original Request

Fork the upstream workmux tool (a git-worktree + tmux orchestration CLI) to support
**"general" tmux sessions** — any directory, any project, no git required — while preserving
all existing worktree functionality unchanged.

The fork lives at `~/indeed/tbrezinski/workmux`.
The user's GitHub fork is at https://github.com/thomasbrezinski/workmux

---

## Plan Summary

Add a new `workmux start <name>` command that:
- Creates a tmux window or session in any directory (no git required)
- Uses the same pane layout as `workmux add` (respects `~/.config/workmux/config.yaml`)
- Registers in the state store so the dashboard sees it immediately
- Explicitly disables post-create hooks and file ops (those are repo-specific)

Extend `close` and `remove` to fall back gracefully when not in a git repo — just kill
the tmux window/session by prefixed name.

Add a **Repo** column to the dashboard (blank for general sessions, repo dir name for
git worktree sessions).

Extend `workmux setup` to offer installing a `Ctrl-b Ctrl-c` keybinding that opens
the dashboard as a tmux popup.

The git dependency is concentrated in 3 places; the hooks/state/dashboard/multiplexer
infrastructure is completely git-free.

### Key architectural insight
- `WorkflowContext::new()` hard-fails outside a git repo (line 39 of context.rs)
- Solution: add `WorkflowContext::new_general()` that skips all git checks
- `setup_environment()` in setup.rs called `get_main_worktree_root()` unconditionally —
  changed to fall back to `worktree_path` for non-git sessions

---

## Execution Record

### Completed

#### Code changes — all on branch `general-sessions` (commit `cb4065c`)

| File | Change |
|------|--------|
| `src/workflow/context.rs` | Added `WorkflowContext::new_general(working_dir, config, mux)` — skips git checks, sets `main_worktree_root`/`git_common_dir` to `working_dir`, `main_branch` to `""` |
| `src/workflow/create.rs` | Added `create_general_session(name, working_dir, context, options, agent)` — skips all git ops (no branch, no worktree, no git config), calls `setup_environment` directly |
| `src/workflow/setup.rs` | Line 51: `get_main_worktree_root()` now `.unwrap_or_else(\|_\| worktree_path.to_path_buf())` instead of hard-failing |
| `src/workflow/mod.rs` | Exports `create_general_session` |
| `src/command/start.rs` | **New file** — implements `workmux start` subcommand |
| `src/command/mod.rs` | Added `pub mod start;` |
| `src/cli.rs` | Added `Start` variant to `Commands` enum; added to `should_prompt_nerdfont` and `should_prompt_status_setup`; added dispatch in `run()` |
| `src/command/close.rs` | For explicit name: tries `git::find_worktree()`, proceeds whether it succeeds or fails (general session just uses mux by prefixed name — no validation needed) |
| `src/command/remove.rs` | Added `MuxHandle` import; `run_specified` now routes non-git names to `remove_general_session()`; added `remove_general_session()` helper that tries Window then Session mode; `remove_worktree()` has fallback to `new_general` context |
| `src/command/dashboard/app.rs` | Added `get_repo_root_for_agent()` accessor for `repo_roots` HashMap |
| `src/command/dashboard/ui/dashboard.rs` | Added "Repo" column after "Project": shows `repo_roots` last path component for git sessions, blank for general sessions |
| `src/command/setup.rs` | After agent hooks install, offers to append `bind-key C-c display-popup -E -w 90% -h 90% "workmux dashboard"` to `~/.tmux.conf` (checks for duplicates) |

#### Build & test
- `cargo build` — clean ✓
- `cargo test` — 597 passed, 0 failed ✓

#### Installation
- Homebrew upstream uninstalled: `brew uninstall workmux`
- Fork installed to `~/.local/bin/workmux` via:
  ```
  cargo install --root ~/.local --path ~/indeed/tbrezinski/workmux
  ```
  (Used `--root ~/.local` because `~/.cargo/bin` is not in PATH but `~/.local/bin` is)
- `which workmux` → `/Users/tbrezinski/.local/bin/workmux` ✓
- `workmux --version` → `0.1.122` ✓
- `workmux start --help` shows the new command ✓

#### Git / GitHub
- Branch: `general-sessions` (local and pushed to fork)
- Remotes:
  - `origin` → https://github.com/raine/workmux.git (upstream)
  - `fork`   → https://github.com/thomasbrezinski/workmux.git (your fork)
- PR #1 open: https://github.com/thomasbrezinski/workmux/pull/1
  - base: `thomasbrezinski/workmux:main`
  - head: `thomasbrezinski/workmux:general-sessions`

---

### Not Yet Done — Verification Checklist

The plan's verification steps require a live tmux session. User is restarting terminal
before running these.

- [ ] **1. Basic general session**
  ```bash
  workmux start test1 --dir ~/
  ```
  Expected: tmux window `wm-test1` created, claude pane opens

- [ ] **2. Status hook transitions**
  Interact with claude in `wm-test1`, verify dashboard shows:
  `working` → `waiting` → `done` status transitions

- [ ] **3. Dashboard Repo column**
  ```bash
  workmux dashboard
  ```
  Expected: `test1` row has blank Repo column; any active worktree sessions
  show the git repo dir name (e.g., `workmux`)

- [ ] **4. Ctrl-b Ctrl-c keybinding** (requires setup first)
  ```bash
  workmux setup
  # Answer Y to the keybinding prompt
  tmux source ~/.tmux.conf
  ```
  Expected: `Ctrl-b Ctrl-c` opens dashboard as popup from any tmux window

- [ ] **5. Lifecycle — close**
  ```bash
  workmux close test1
  ```
  Expected: window killed, no errors about missing git repo

- [ ] **6. Lifecycle — remove**
  ```bash
  workmux start test2 --dir ~/tmp
  workmux remove test2
  ```
  Expected: window killed, no errors

- [ ] **7. Session mode**
  ```bash
  workmux start test3 --dir ~/ --session
  ```
  Expected: dedicated tmux session `wm-test3` created (not a window)

- [ ] **8. Regression — workmux add still works**
  From inside a git repo:
  ```bash
  workmux add some-branch
  ```
  Expected: behaves exactly as before

---

## Key File Locations

| Thing | Path |
|-------|------|
| Fork source | `~/indeed/tbrezinski/workmux/` |
| Installed binary | `~/.local/bin/workmux` |
| Reinstall command | `cargo install --root ~/.local --path ~/indeed/tbrezinski/workmux` |
| PR | https://github.com/thomasbrezinski/workmux/pull/1 |
| Working branch | `general-sessions` |

## Notes for Next Session

- The terminal restart is to pick up PATH or env changes unrelated to workmux
- After restart, `workmux` should still resolve to `~/.local/bin/workmux` (it's in PATH via `~/.local/bin`)
- If it doesn't, run: `cargo install --root ~/.local --path ~/indeed/tbrezinski/workmux`
- All verification steps require being inside a running tmux session
- `workmux setup` will also re-run agent hook installation — that's fine, it's idempotent
