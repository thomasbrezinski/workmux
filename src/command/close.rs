use crate::multiplexer::handle::mode_label;
use crate::multiplexer::{MuxHandle, create_backend, detect_backend};
use crate::{config, git, sandbox};
use anyhow::{Context, Result, anyhow};

pub fn run(name: Option<&str>) -> Result<()> {
    let config = config::Config::load(None)?;
    let mux = create_backend(detect_backend());
    let prefix = config.window_prefix();

    // Resolve the handle first to determine target mode
    let resolved_handle = match name {
        Some(h) => h.to_string(),
        None => super::resolve_name(None)?,
    };

    // Determine if this worktree was created as a session or window.
    // Fall back to config default for general (non-git) sessions.
    let mode = git::get_worktree_mode(&resolved_handle);

    // When no name is provided, prefer the current window/session name
    // This handles duplicate windows/sessions (e.g., wm:feature-2) correctly
    let (full_target_name, is_current_target) = match name {
        Some(handle) => {
            // Check if this is a git worktree first; if not, treat as general session.
            let is_git_worktree = git::find_worktree(handle).is_ok();
            if !is_git_worktree {
                // General session: just look for the mux window/session by prefixed name
                let target = MuxHandle::new(mux.as_ref(), mode, prefix, handle);
                let full = target.full_name();
                let current = target.current_name()?;
                let is_current = current.as_deref() == Some(full.as_str());
                (full, is_current)
            } else {
                let target = MuxHandle::new(mux.as_ref(), mode, prefix, handle);
                let full = target.full_name();
                let current = target.current_name()?;
                let is_current = current.as_deref() == Some(full.as_str());
                (full, is_current)
            }
        }
        None => {
            // No name provided - check if we're in a workmux window/session
            let target = MuxHandle::new(mux.as_ref(), mode, prefix, &resolved_handle);
            let current_name = target.current_name()?;
            if let Some(current) = current_name {
                if current.starts_with(prefix) {
                    // We're in a workmux target, use it directly
                    (current.clone(), true)
                } else {
                    // Not in a workmux target, fall back to resolved handle
                    (target.full_name(), false)
                }
            } else {
                // Not in multiplexer, use resolved handle
                (target.full_name(), false)
            }
        }
    };

    let kind = mode_label(mode);
    let target_exists = MuxHandle::exists_full(mux.as_ref(), mode, &full_target_name)?;

    if !target_exists {
        return Err(anyhow!(
            "No active {} found for '{}'. The worktree exists but has no open {}.",
            kind,
            full_target_name,
            kind
        ));
    }

    // Manifest: snapshot pane title and Claude session ID before killing the window.
    if let Some(handle) = full_target_name.strip_prefix(prefix) {
        if let Ok(mstore) = crate::manifest::ManifestStore::new() {
            let repo_root = git::get_repo_root().ok();
            let _ = mstore.update_entry(repo_root.as_deref(), handle, |entry| {
                // Capture pane title from the agent state or tmux pane
                if let Ok(agents) = crate::state::StateStore::new()
                    .and_then(|s| s.list_all_agents())
                {
                    // Find agent whose window matches this handle
                    for agent in &agents {
                        if agent.workdir == entry.workdir {
                            if let Some(ref title) = agent.pane_title {
                                entry.last_pane_title = Some(title.clone());
                            }
                            if let Some(status) = agent.status {
                                entry.last_agent_status = Some(
                                    match status {
                                        crate::multiplexer::AgentStatus::Working => "working",
                                        crate::multiplexer::AgentStatus::Waiting => "waiting",
                                        crate::multiplexer::AgentStatus::Done => "done",
                                    }
                                    .to_string(),
                                );
                            }
                            break;
                        }
                    }
                }
                entry.updated_at = crate::manifest::unix_now();

                // Also capture Claude session ID on close
                if let Some(id) =
                    crate::manifest::claude::capture_claude_session_id(&entry.workdir)
                {
                    entry.claude_session_id = Some(id);
                }
            });
            // Try without repo_root too (general session)
            if repo_root.is_some() {
                let _ = mstore.update_entry(None, handle, |entry| {
                    entry.updated_at = crate::manifest::unix_now();
                    if let Some(id) =
                        crate::manifest::claude::capture_claude_session_id(&entry.workdir)
                    {
                        entry.claude_session_id = Some(id);
                    }
                });
            }
        }
    }

    // Stop any running containers for this worktree before killing the target.
    if let Some(handle) = full_target_name.strip_prefix(prefix) {
        sandbox::stop_containers_for_handle(handle, &config.sandbox);
    }

    if is_current_target {
        let delay = std::time::Duration::from_millis(100);
        MuxHandle::schedule_close_full(mux.as_ref(), mode, &full_target_name, delay)?;
    } else {
        MuxHandle::kill_full(mux.as_ref(), mode, &full_target_name)
            .context("Failed to close target")?;
        println!("✓ Closed {} '{}' (worktree kept)", kind, full_target_name);
    }

    Ok(())
}
