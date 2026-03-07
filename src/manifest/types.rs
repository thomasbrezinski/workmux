//! Core data structures for the session manifest.
//!
//! The manifest is a durable record of all workmux sessions (worktree and
//! general) stored at `~/.local/state/workmux/manifest.json`. It survives
//! tmux restarts and computer reboots, unlike the ephemeral agent state files.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Top-level manifest structure.
///
/// Uses a `BTreeMap` for deterministic key ordering in the JSON output,
/// making the file easier to inspect and diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Schema version for future migrations. Currently `1`.
    pub version: u32,

    /// Map of composite key → session entry.
    /// Key format: `"{repo_root}::{handle}"` or `"::{handle}"` for general sessions.
    pub sessions: BTreeMap<String, ManifestEntry>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            sessions: BTreeMap::new(),
        }
    }
}

/// A single session record in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Short name used as the tmux window/session name (e.g., "my-feature").
    pub handle: String,

    /// Whether this is a git worktree or a general (directory-only) session.
    #[serde(rename = "type")]
    pub session_type: SessionType,

    /// Current lifecycle state: active or archived.
    pub lifecycle: Lifecycle,

    /// Absolute path to the session's working directory.
    pub workdir: PathBuf,

    /// Git repository root (for worktree sessions). `None` for general sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<PathBuf>,

    /// Git branch name (for worktree sessions). `None` for general sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    /// Unix timestamp when the session was first created.
    pub created_at: u64,

    /// Unix timestamp of the most recent update (status change, close, etc.).
    pub updated_at: u64,

    /// Unix timestamp when the session was archived. `None` if active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<u64>,

    /// Claude Code session UUID for resuming conversations.
    /// Captured from `~/.claude.json` on Stop hook or close.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,

    /// Human-readable Claude session slug (e.g., "eventual-bubbling-music").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_session_slug: Option<String>,

    /// Last known agent status snapshot: "working", "waiting", or "done".
    /// Authoritative when no live agent state exists; overridden by live data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_agent_status: Option<String>,

    /// Last known pane title / Claude conversation summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pane_title: Option<String>,
}

/// Session type discriminator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionType {
    /// Git worktree-backed session (`workmux add`).
    Worktree,
    /// Directory-only session (`workmux start`).
    General,
}

/// Session lifecycle state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    /// Visible in dashboard and list.
    Active,
    /// Hidden from default views, still queryable and resumable.
    Archived,
}

impl ManifestEntry {
    /// Create a new worktree session entry.
    pub fn new_worktree(
        handle: &str,
        workdir: &Path,
        repo_root: Option<&Path>,
        branch: &str,
    ) -> Self {
        let now = unix_now();
        Self {
            handle: handle.to_string(),
            session_type: SessionType::Worktree,
            lifecycle: Lifecycle::Active,
            workdir: workdir.to_path_buf(),
            repo_root: repo_root.map(|p| p.to_path_buf()),
            branch: Some(branch.to_string()),
            created_at: now,
            updated_at: now,
            archived_at: None,
            claude_session_id: None,
            claude_session_slug: None,
            last_agent_status: None,
            last_pane_title: None,
        }
    }

    /// Create a new general session entry.
    pub fn new_general(handle: &str, workdir: &Path) -> Self {
        let now = unix_now();
        Self {
            handle: handle.to_string(),
            session_type: SessionType::General,
            lifecycle: Lifecycle::Active,
            workdir: workdir.to_path_buf(),
            repo_root: None,
            branch: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            claude_session_id: None,
            claude_session_slug: None,
            last_agent_status: None,
            last_pane_title: None,
        }
    }
}

/// Build the composite manifest key for a session.
///
/// Format: `"{repo_root}::{handle}"` for worktree sessions,
/// `"::{handle}"` for general sessions (no repo root).
pub fn manifest_key(repo_root: Option<&Path>, handle: &str) -> String {
    match repo_root {
        Some(root) => format!("{}::{}", root.display(), handle),
        None => format!("::{}", handle),
    }
}

/// Current unix timestamp in seconds.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_key_with_repo() {
        let key = manifest_key(Some(Path::new("/Users/user/project")), "my-feature");
        assert_eq!(key, "/Users/user/project::my-feature");
    }

    #[test]
    fn test_manifest_key_without_repo() {
        let key = manifest_key(None, "explore-caching");
        assert_eq!(key, "::explore-caching");
    }

    #[test]
    fn test_new_worktree_entry() {
        let entry = ManifestEntry::new_worktree(
            "feat",
            Path::new("/project__worktrees/feat"),
            Some(Path::new("/project")),
            "feat-branch",
        );
        assert_eq!(entry.handle, "feat");
        assert_eq!(entry.session_type, SessionType::Worktree);
        assert_eq!(entry.lifecycle, Lifecycle::Active);
        assert_eq!(entry.branch.as_deref(), Some("feat-branch"));
        assert!(entry.repo_root.is_some());
        assert!(entry.archived_at.is_none());
        assert!(entry.created_at > 0);
        assert_eq!(entry.created_at, entry.updated_at);
    }

    #[test]
    fn test_new_general_entry() {
        let entry = ManifestEntry::new_general("explore", Path::new("/Users/user/Work"));
        assert_eq!(entry.handle, "explore");
        assert_eq!(entry.session_type, SessionType::General);
        assert_eq!(entry.lifecycle, Lifecycle::Active);
        assert!(entry.branch.is_none());
        assert!(entry.repo_root.is_none());
    }

    #[test]
    fn test_default_manifest() {
        let m = Manifest::default();
        assert_eq!(m.version, 1);
        assert!(m.sessions.is_empty());
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut m = Manifest::default();
        m.sessions.insert(
            "/project::feat".to_string(),
            ManifestEntry::new_worktree(
                "feat",
                Path::new("/project__worktrees/feat"),
                Some(Path::new("/project")),
                "feat",
            ),
        );
        m.sessions.insert(
            "::general-task".to_string(),
            ManifestEntry::new_general("general-task", Path::new("/tmp/work")),
        );

        let json = serde_json::to_string_pretty(&m).unwrap();
        let parsed: Manifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.sessions.len(), 2);
        assert!(parsed.sessions.contains_key("/project::feat"));
        assert!(parsed.sessions.contains_key("::general-task"));

        let wt = &parsed.sessions["/project::feat"];
        assert_eq!(wt.session_type, SessionType::Worktree);

        let general = &parsed.sessions["::general-task"];
        assert_eq!(general.session_type, SessionType::General);
    }

    #[test]
    fn test_lifecycle_serde() {
        let active = serde_json::to_string(&Lifecycle::Active).unwrap();
        assert_eq!(active, "\"active\"");

        let archived = serde_json::to_string(&Lifecycle::Archived).unwrap();
        assert_eq!(archived, "\"archived\"");

        let parsed: Lifecycle = serde_json::from_str("\"archived\"").unwrap();
        assert_eq!(parsed, Lifecycle::Archived);
    }

    #[test]
    fn test_session_type_serde() {
        let wt = serde_json::to_string(&SessionType::Worktree).unwrap();
        assert_eq!(wt, "\"worktree\"");

        let general = serde_json::to_string(&SessionType::General).unwrap();
        assert_eq!(general, "\"general\"");
    }
}
