//! Filesystem-based persistence for the session manifest.
//!
//! Stores the manifest at `~/.local/state/workmux/manifest.json` using
//! the same XDG state directory and atomic-write pattern as the agent
//! state store.

use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::warn;

use super::types::{Lifecycle, Manifest, ManifestEntry, manifest_key, unix_now};

/// Manages the session manifest file.
pub struct ManifestStore {
    path: PathBuf,
}

impl ManifestStore {
    /// Create a new ManifestStore using the XDG state directory.
    ///
    /// The manifest file lives at `~/.local/state/workmux/manifest.json`.
    /// Creates the parent directory if it doesn't exist.
    pub fn new() -> Result<Self> {
        let base = crate::state::store::get_state_dir()?.join("workmux");
        fs::create_dir_all(&base).context("Failed to create state directory")?;
        Ok(Self {
            path: base.join("manifest.json"),
        })
    }

    /// Create a ManifestStore with a custom path (for testing).
    #[cfg(test)]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load the manifest from disk.
    ///
    /// Returns a default empty manifest if the file doesn't exist.
    /// Deletes and returns default if the file is corrupted.
    pub fn load(&self) -> Result<Manifest> {
        match fs::read_to_string(&self.path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(manifest) => Ok(manifest),
                Err(e) => {
                    warn!(?self.path, error = %e, "corrupted manifest file, resetting");
                    Ok(Manifest::default())
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(e).context("Failed to read manifest"),
        }
    }

    /// Save the manifest to disk using atomic write (tmp + rename).
    pub fn save(&self, manifest: &Manifest) -> Result<()> {
        let content = serde_json::to_string_pretty(manifest)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, content.as_bytes()).context("Failed to write manifest temp file")?;
        fs::rename(&tmp, &self.path).context("Failed to rename manifest temp file")?;
        Ok(())
    }

    /// Insert or update a manifest entry.
    ///
    /// If the entry already exists, it is replaced entirely.
    pub fn upsert_entry(
        &self,
        repo_root: Option<&Path>,
        handle: &str,
        entry: ManifestEntry,
    ) -> Result<()> {
        let mut manifest = self.load()?;
        let key = manifest_key(repo_root, handle);
        manifest.sessions.insert(key, entry);
        self.save(&manifest)
    }

    /// Get a manifest entry by repo root and handle.
    pub fn get_entry(
        &self,
        repo_root: Option<&Path>,
        handle: &str,
    ) -> Result<Option<ManifestEntry>> {
        let manifest = self.load()?;
        let key = manifest_key(repo_root, handle);
        Ok(manifest.sessions.get(&key).cloned())
    }

    /// Remove a manifest entry.
    ///
    /// No-op if the entry doesn't exist.
    pub fn remove_entry(&self, repo_root: Option<&Path>, handle: &str) -> Result<()> {
        let mut manifest = self.load()?;
        let key = manifest_key(repo_root, handle);
        if manifest.sessions.remove(&key).is_some() {
            self.save(&manifest)?;
        }
        Ok(())
    }

    /// Update a manifest entry by composite key, applying a closure.
    ///
    /// No-op if the entry doesn't exist.
    pub fn update_entry_by_key(
        &self,
        key: &str,
        f: impl FnOnce(&mut ManifestEntry),
    ) -> Result<bool> {
        let mut manifest = self.load()?;
        if let Some(entry) = manifest.sessions.get_mut(key) {
            f(entry);
            self.save(&manifest)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Update a manifest entry by repo root + handle, applying a closure.
    ///
    /// No-op if the entry doesn't exist. Returns whether an entry was updated.
    pub fn update_entry(
        &self,
        repo_root: Option<&Path>,
        handle: &str,
        f: impl FnOnce(&mut ManifestEntry),
    ) -> Result<bool> {
        let key = manifest_key(repo_root, handle);
        self.update_entry_by_key(&key, f)
    }

    /// Find and update a manifest entry by matching workdir path.
    ///
    /// Used by `persist_agent_update()` where we have the workdir but not
    /// the repo_root + handle key. Scans all entries for a workdir match.
    ///
    /// Returns whether an entry was found and updated.
    pub fn update_by_workdir(
        &self,
        workdir: &Path,
        f: impl FnOnce(&mut ManifestEntry),
    ) -> Result<bool> {
        let mut manifest = self.load()?;

        // Find the entry whose workdir matches (or is a parent of) the given path.
        // Exact match first, then prefix match.
        let key = manifest
            .sessions
            .iter()
            .find(|(_, e)| e.workdir == workdir)
            .or_else(|| {
                manifest
                    .sessions
                    .iter()
                    .find(|(_, e)| workdir.starts_with(&e.workdir))
            })
            .map(|(k, _)| k.clone());

        if let Some(key) = key {
            if let Some(entry) = manifest.sessions.get_mut(&key) {
                f(entry);
                self.save(&manifest)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// List manifest entries, optionally filtered by lifecycle state.
    ///
    /// Returns `(key, entry)` pairs in key order.
    pub fn list_entries(
        &self,
        lifecycle_filter: Option<Lifecycle>,
    ) -> Result<Vec<(String, ManifestEntry)>> {
        let manifest = self.load()?;
        let entries = manifest
            .sessions
            .into_iter()
            .filter(|(_, e)| match lifecycle_filter {
                Some(lc) => e.lifecycle == lc,
                None => true,
            })
            .collect();
        Ok(entries)
    }

    /// Archive a session by setting lifecycle to Archived.
    ///
    /// Returns `Ok(true)` if the entry was found and archived,
    /// `Ok(false)` if no matching entry exists.
    pub fn archive(&self, repo_root: Option<&Path>, handle: &str) -> Result<bool> {
        let now = unix_now();
        self.update_entry(repo_root, handle, |entry| {
            entry.lifecycle = Lifecycle::Archived;
            entry.archived_at = Some(now);
            entry.updated_at = now;
        })
    }

    /// Unarchive a session by setting lifecycle back to Active.
    ///
    /// Returns `Ok(true)` if the entry was found and unarchived.
    pub fn unarchive(&self, repo_root: Option<&Path>, handle: &str) -> Result<bool> {
        let now = unix_now();
        self.update_entry(repo_root, handle, |entry| {
            entry.lifecycle = Lifecycle::Active;
            entry.archived_at = None;
            entry.updated_at = now;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn test_store() -> (ManifestStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("manifest.json");
        let store = ManifestStore::with_path(path);
        (store, dir)
    }

    #[test]
    fn test_load_missing_file() {
        let (store, _dir) = test_store();
        let manifest = store.load().unwrap();
        assert_eq!(manifest.version, 1);
        assert!(manifest.sessions.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let (store, _dir) = test_store();

        let entry = ManifestEntry::new_worktree(
            "feat",
            Path::new("/project__worktrees/feat"),
            Some(Path::new("/project")),
            "feat-branch",
        );
        store
            .upsert_entry(Some(Path::new("/project")), "feat", entry)
            .unwrap();

        let manifest = store.load().unwrap();
        assert_eq!(manifest.sessions.len(), 1);
        assert!(manifest.sessions.contains_key("/project::feat"));
    }

    #[test]
    fn test_upsert_replaces_existing() {
        let (store, _dir) = test_store();

        let entry1 = ManifestEntry::new_general("task", Path::new("/work"));
        store.upsert_entry(None, "task", entry1).unwrap();

        let mut entry2 = ManifestEntry::new_general("task", Path::new("/work"));
        entry2.last_pane_title = Some("Updated title".to_string());
        store.upsert_entry(None, "task", entry2).unwrap();

        let manifest = store.load().unwrap();
        assert_eq!(manifest.sessions.len(), 1);
        assert_eq!(
            manifest.sessions["::task"].last_pane_title.as_deref(),
            Some("Updated title")
        );
    }

    #[test]
    fn test_get_entry() {
        let (store, _dir) = test_store();

        let entry = ManifestEntry::new_general("explore", Path::new("/work"));
        store.upsert_entry(None, "explore", entry).unwrap();

        let found = store.get_entry(None, "explore").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().handle, "explore");

        let missing = store.get_entry(None, "nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_remove_entry() {
        let (store, _dir) = test_store();

        let entry = ManifestEntry::new_general("task", Path::new("/work"));
        store.upsert_entry(None, "task", entry).unwrap();

        store.remove_entry(None, "task").unwrap();

        let manifest = store.load().unwrap();
        assert!(manifest.sessions.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_is_noop() {
        let (store, _dir) = test_store();
        // Should not error
        store.remove_entry(None, "nonexistent").unwrap();
    }

    #[test]
    fn test_update_entry() {
        let (store, _dir) = test_store();

        let entry = ManifestEntry::new_general("task", Path::new("/work"));
        store.upsert_entry(None, "task", entry).unwrap();

        let updated = store
            .update_entry(None, "task", |e| {
                e.last_agent_status = Some("done".to_string());
                e.last_pane_title = Some("Finished work".to_string());
            })
            .unwrap();
        assert!(updated);

        let found = store.get_entry(None, "task").unwrap().unwrap();
        assert_eq!(found.last_agent_status.as_deref(), Some("done"));
        assert_eq!(found.last_pane_title.as_deref(), Some("Finished work"));
    }

    #[test]
    fn test_update_entry_missing_returns_false() {
        let (store, _dir) = test_store();

        let updated = store
            .update_entry(None, "nonexistent", |e| {
                e.last_agent_status = Some("done".to_string());
            })
            .unwrap();
        assert!(!updated);
    }

    #[test]
    fn test_update_by_workdir() {
        let (store, _dir) = test_store();

        let entry = ManifestEntry::new_general("task", Path::new("/Users/user/Work"));
        store.upsert_entry(None, "task", entry).unwrap();

        let updated = store
            .update_by_workdir(Path::new("/Users/user/Work"), |e| {
                e.last_agent_status = Some("working".to_string());
            })
            .unwrap();
        assert!(updated);

        let found = store.get_entry(None, "task").unwrap().unwrap();
        assert_eq!(found.last_agent_status.as_deref(), Some("working"));
    }

    #[test]
    fn test_update_by_workdir_no_match() {
        let (store, _dir) = test_store();

        let entry = ManifestEntry::new_general("task", Path::new("/work"));
        store.upsert_entry(None, "task", entry).unwrap();

        let updated = store
            .update_by_workdir(Path::new("/totally/different"), |e| {
                e.last_agent_status = Some("done".to_string());
            })
            .unwrap();
        assert!(!updated);
    }

    #[test]
    fn test_list_entries_all() {
        let (store, _dir) = test_store();

        store
            .upsert_entry(
                None,
                "active-task",
                ManifestEntry::new_general("active-task", Path::new("/work")),
            )
            .unwrap();

        let mut archived = ManifestEntry::new_general("old-task", Path::new("/work2"));
        archived.lifecycle = Lifecycle::Archived;
        archived.archived_at = Some(1000);
        store.upsert_entry(None, "old-task", archived).unwrap();

        let all = store.list_entries(None).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_list_entries_filtered() {
        let (store, _dir) = test_store();

        store
            .upsert_entry(
                None,
                "active-task",
                ManifestEntry::new_general("active-task", Path::new("/work")),
            )
            .unwrap();

        let mut archived = ManifestEntry::new_general("old-task", Path::new("/work2"));
        archived.lifecycle = Lifecycle::Archived;
        store.upsert_entry(None, "old-task", archived).unwrap();

        let active = store.list_entries(Some(Lifecycle::Active)).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].1.handle, "active-task");

        let arch = store.list_entries(Some(Lifecycle::Archived)).unwrap();
        assert_eq!(arch.len(), 1);
        assert_eq!(arch[0].1.handle, "old-task");
    }

    #[test]
    fn test_archive_and_unarchive() {
        let (store, _dir) = test_store();

        let entry = ManifestEntry::new_general("task", Path::new("/work"));
        store.upsert_entry(None, "task", entry).unwrap();

        // Archive
        let ok = store.archive(None, "task").unwrap();
        assert!(ok);

        let found = store.get_entry(None, "task").unwrap().unwrap();
        assert_eq!(found.lifecycle, Lifecycle::Archived);
        assert!(found.archived_at.is_some());

        // Unarchive
        let ok = store.unarchive(None, "task").unwrap();
        assert!(ok);

        let found = store.get_entry(None, "task").unwrap().unwrap();
        assert_eq!(found.lifecycle, Lifecycle::Active);
        assert!(found.archived_at.is_none());
    }

    #[test]
    fn test_archive_nonexistent_returns_false() {
        let (store, _dir) = test_store();
        let ok = store.archive(None, "nonexistent").unwrap();
        assert!(!ok);
    }

    #[test]
    fn test_atomic_write_no_tmp_files() {
        let (store, dir) = test_store();

        let entry = ManifestEntry::new_general("task", Path::new("/work"));
        store.upsert_entry(None, "task", entry).unwrap();

        // Check no .tmp files remain in the directory
        for f in fs::read_dir(dir.path()).unwrap() {
            let name = f.unwrap().file_name().to_string_lossy().to_string();
            assert!(!name.ends_with(".tmp"), "temp file should be cleaned up");
        }
    }

    #[test]
    fn test_corrupted_file_resets() {
        let (store, _dir) = test_store();

        // Write garbage
        fs::write(&store.path, "not valid json {{{").unwrap();

        // Should return default, not error
        let manifest = store.load().unwrap();
        assert_eq!(manifest.version, 1);
        assert!(manifest.sessions.is_empty());
    }

    #[test]
    fn test_multiple_sessions_different_repos() {
        let (store, _dir) = test_store();

        store
            .upsert_entry(
                Some(Path::new("/repo-a")),
                "feat",
                ManifestEntry::new_worktree(
                    "feat",
                    Path::new("/repo-a__worktrees/feat"),
                    Some(Path::new("/repo-a")),
                    "feat",
                ),
            )
            .unwrap();

        store
            .upsert_entry(
                Some(Path::new("/repo-b")),
                "feat",
                ManifestEntry::new_worktree(
                    "feat",
                    Path::new("/repo-b__worktrees/feat"),
                    Some(Path::new("/repo-b")),
                    "feat",
                ),
            )
            .unwrap();

        store
            .upsert_entry(
                None,
                "general-task",
                ManifestEntry::new_general("general-task", Path::new("/work")),
            )
            .unwrap();

        let manifest = store.load().unwrap();
        assert_eq!(manifest.sessions.len(), 3);
        assert!(manifest.sessions.contains_key("/repo-a::feat"));
        assert!(manifest.sessions.contains_key("/repo-b::feat"));
        assert!(manifest.sessions.contains_key("::general-task"));
    }
}
