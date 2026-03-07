//! Filesystem-based state storage for workmux agents.
//!
//! This module provides persistent state storage that works across all
//! terminal multiplexer backends (tmux, WezTerm, Zellij).

pub mod run;
pub(crate) mod store;
mod types;

use std::time::{SystemTime, UNIX_EPOCH};

use tracing::warn;

use crate::multiplexer::{AgentStatus, Multiplexer};

pub use store::StateStore;
pub use types::{AgentState, PaneKey};

/// Persist an agent state update to the StateStore.
///
/// Merges with existing state so partial updates don't wipe other fields:
/// - If `status` is Some, updates the agent's status. If None, preserves existing.
/// - If `title_override` is Some, uses it. If None, preserves existing stored title,
///   falling back to the live pane title.
///
/// Logs warnings on failure without propagating errors (best-effort persistence).
pub fn persist_agent_update(
    mux: &dyn Multiplexer,
    pane_id: &str,
    status: Option<AgentStatus>,
    title_override: Option<String>,
) {
    let pane_key = PaneKey {
        backend: mux.name().to_string(),
        instance: mux.instance_id(),
        pane_id: pane_id.to_string(),
    };

    let live_info = match mux.get_live_pane_info(pane_id) {
        Ok(Some(info)) => info,
        Ok(None) => {
            warn!(%pane_id, "pane not found, skipping state persist");
            return;
        }
        Err(e) => {
            warn!(error = %e, "failed to get live pane info, skipping state persist");
            return;
        }
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Load existing state to merge with
    let existing = StateStore::new()
        .ok()
        .and_then(|store| store.get_agent(&pane_key).ok().flatten());

    // Resolve status: explicit update wins, otherwise preserve existing
    let final_status = status.or(existing.as_ref().and_then(|e| e.status));

    // Preserve existing status_ts if status hasn't changed (avoids resetting timer)
    let status_ts = if final_status == existing.as_ref().and_then(|e| e.status) {
        existing.as_ref().and_then(|e| e.status_ts).unwrap_or(now)
    } else {
        now
    };

    // Resolve title: explicit override wins, then existing stored title, then live
    let pane_title = title_override
        .or(existing.and_then(|e| e.pane_title))
        .or(live_info.title);

    let state = AgentState {
        pane_key,
        workdir: live_info.working_dir,
        status: final_status,
        status_ts: Some(status_ts),
        pane_title,
        pane_pid: live_info.pid,
        command: live_info.current_command,
        updated_ts: now,
    };

    if let Ok(store) = StateStore::new()
        && let Err(e) = store.upsert_agent(&state)
    {
        warn!(error = %e, "failed to persist agent state");
    }

    // Manifest: update agent status snapshot (fire-and-forget).
    // On "done" status, also capture the Claude session ID.
    if let Ok(mstore) = crate::manifest::ManifestStore::new() {
        let status_str = status.map(|s| match s {
            AgentStatus::Working => "working",
            AgentStatus::Waiting => "waiting",
            AgentStatus::Done => "done",
        });
        let is_done = status_str == Some("done");
        let workdir = &state.workdir;
        let pane_title = state.pane_title.clone();

        let _ = mstore.update_by_workdir(workdir, |entry| {
            // Debounce: skip if status unchanged
            if let Some(s) = status_str {
                if entry.last_agent_status.as_deref() != Some(s) {
                    entry.last_agent_status = Some(s.to_string());
                }
            }
            if let Some(title) = pane_title {
                entry.last_pane_title = Some(title);
            }
            entry.updated_at = now;

            // On "done": capture Claude session ID from ~/.claude.json
            if is_done {
                if let Some(id) = crate::manifest::claude::capture_claude_session_id(&entry.workdir)
                {
                    entry.claude_session_id = Some(id);
                }
            }
        });
    }
}
