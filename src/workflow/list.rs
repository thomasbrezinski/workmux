use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::config::MuxMode;
use crate::multiplexer::{Multiplexer, util};
use crate::state::StateStore;
use crate::util::canon_or_self;
use crate::{config, git, github, spinner};

use super::types::{AgentStatusSummary, WorktreeInfo};

/// Filter worktrees by handle (directory name) or branch name.
/// Uses handle-first precedence: if a filter token matches a handle, that takes
/// priority over branch name matches.
fn filter_worktrees(
    worktrees: Vec<(PathBuf, String)>,
    filter: &[String],
) -> Vec<(PathBuf, String)> {
    if filter.is_empty() {
        return worktrees;
    }

    let mut matched_paths = HashSet::new();

    for token in filter {
        // First: try to match by handle (directory name)
        let handle_match = worktrees.iter().find(|(path, _)| {
            path.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| name == token)
        });

        if let Some((path, _)) = handle_match {
            matched_paths.insert(path.clone());
            continue;
        }

        // Fallback: try to match by branch name
        for (path, branch) in &worktrees {
            if branch == token {
                matched_paths.insert(path.clone());
            }
        }
    }

    worktrees
        .into_iter()
        .filter(|(path, _)| matched_paths.contains(path))
        .collect()
}

/// List all worktrees with their status
pub fn list(
    config: &config::Config,
    mux: &dyn Multiplexer,
    fetch_pr_status: bool,
    filter: &[String],
) -> Result<Vec<WorktreeInfo>> {
    // Check mux status first — needed for both git worktrees and general sessions
    let mux_running = mux.is_running().unwrap_or(false);
    let mux_windows: HashSet<String> = if mux_running {
        mux.get_all_window_names().unwrap_or_default()
    } else {
        HashSet::new()
    };
    let mux_sessions: HashSet<String> = if mux_running {
        mux.get_all_session_names().unwrap_or_default()
    } else {
        HashSet::new()
    };

    // Load reconciled agent states (needed for both git worktrees and general sessions)
    let agent_panes = if mux_running {
        StateStore::new()
            .ok()
            .and_then(|store| store.load_reconciled_agents(mux).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Pre-calculate canonical paths for agents to avoid repeated syscalls
    let agent_panes_canon: Vec<_> = agent_panes
        .iter()
        .map(|a| (canon_or_self(&a.path), a.status))
        .collect();

    let prefix = config.window_prefix();
    let mut worktrees: Vec<WorktreeInfo> = Vec::new();

    // Git worktree section — only runs when inside a git repository
    if git::is_git_repo()? {
        let worktrees_data = filter_worktrees(git::list_worktrees()?, filter);

        if !worktrees_data.is_empty() {
            let main_branch = git::get_default_branch().ok();
            let unmerged_branches = main_branch
                .as_deref()
                .and_then(|main| git::get_merge_base(main).ok())
                .and_then(|base| git::get_unmerged_branches(&base).ok())
                .unwrap_or_default();

            let pr_map = if fetch_pr_status {
                spinner::with_spinner("Fetching PR status", || {
                    Ok(github::list_prs().unwrap_or_default())
                })?
            } else {
                std::collections::HashMap::new()
            };

            let worktree_modes = git::get_all_worktree_modes();

            for (path, branch) in worktrees_data {
                let handle = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&branch)
                    .to_string();

                let prefixed_name = util::prefixed(prefix, &handle);
                let mode = worktree_modes
                    .get(&handle)
                    .copied()
                    .unwrap_or(MuxMode::Window);
                let has_mux_window = if mode == MuxMode::Session {
                    mux_sessions.contains(&prefixed_name)
                } else {
                    mux_windows.contains(&prefixed_name)
                };

                let has_unmerged = if let Some(ref main) = main_branch {
                    branch != *main && branch != "(detached)" && unmerged_branches.contains(&branch)
                } else {
                    false
                };

                let pr_info = pr_map.get(&branch).cloned();

                let canon_wt_path = canon_or_self(&path);
                let matching_statuses: Vec<_> = agent_panes_canon
                    .iter()
                    .filter(|(p, _)| *p == canon_wt_path || p.starts_with(&canon_wt_path))
                    .filter_map(|(_, status)| *status)
                    .collect();
                let agent_status = if matching_statuses.is_empty() {
                    None
                } else {
                    Some(AgentStatusSummary { statuses: matching_statuses })
                };

                worktrees.push(WorktreeInfo {
                    branch,
                    path,
                    has_mux_window,
                    has_unmerged,
                    pr_info,
                    agent_status,
                });
            }
        }
    }

    // General sessions: scan live panes for wm-* windows not covered by a git worktree
    if mux_running {
        let covered_handles: HashSet<String> = worktrees
            .iter()
            .map(|wt| {
                wt.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&wt.branch)
                    .to_string()
            })
            .collect();

        if let Ok(live_panes) = mux.get_all_live_pane_info() {
            let mut seen_windows: HashSet<String> = HashSet::new();
            for (_pane_id, info) in live_panes {
                let window_name = info.window.as_deref().unwrap_or("");
                if let Some(handle) = window_name.strip_prefix(prefix) {
                    if !covered_handles.contains(handle)
                        && seen_windows.insert(window_name.to_string())
                    {
                        let canon_path = canon_or_self(&info.working_dir);
                        let matching_statuses: Vec<_> = agent_panes_canon
                            .iter()
                            .filter(|(p, _)| *p == canon_path || p.starts_with(&canon_path))
                            .filter_map(|(_, s)| *s)
                            .collect();
                        let agent_status = if matching_statuses.is_empty() {
                            None
                        } else {
                            Some(AgentStatusSummary { statuses: matching_statuses })
                        };

                        worktrees.push(WorktreeInfo {
                            branch: handle.to_string(),
                            path: info.working_dir,
                            has_mux_window: true,
                            has_unmerged: false,
                            pr_info: None,
                            agent_status,
                        });
                    }
                }
            }
        }
    }

    Ok(worktrees)
}
