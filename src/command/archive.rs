//! Archive/unarchive sessions via the manifest.

use anyhow::{Result, anyhow};

use crate::git;
use crate::manifest::ManifestStore;

pub fn run(name: &str, undo: bool) -> Result<()> {
    let store = ManifestStore::new()?;

    // Build the key: try with repo_root first (if inside a git repo), then without
    let repo_root = git::get_repo_root().ok();

    if undo {
        let found = store.unarchive(repo_root.as_deref(), name)?;
        if !found {
            // Try without repo root (might be a general session)
            let found_general = if repo_root.is_some() {
                store.unarchive(None, name)?
            } else {
                false
            };
            if !found_general {
                return Err(anyhow!("No manifest entry found for '{}'", name));
            }
        }
        println!("Unarchived '{}'", name);
    } else {
        let found = store.archive(repo_root.as_deref(), name)?;
        if !found {
            let found_general = if repo_root.is_some() {
                store.archive(None, name)?
            } else {
                false
            };
            if !found_general {
                return Err(anyhow!("No manifest entry found for '{}'", name));
            }
        }
        println!("Archived '{}'", name);
    }

    Ok(())
}
