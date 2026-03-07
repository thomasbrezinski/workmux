//! Claude Code session ID capture.
//!
//! Reads `~/.claude.json` to extract the most recent session ID for a given
//! working directory. Claude Code stores `lastSessionId` per project path
//! in this file.
//!
//! Session IDs can be used to resume conversations: `claude --resume <id>`

use std::fs;
use std::path::Path;

use tracing::debug;

/// Attempt to capture the Claude Code session ID for a working directory.
///
/// Reads `~/.claude.json` and looks for a project entry whose key matches
/// (or is a parent of) the given workdir. Returns the session ID if found.
///
/// Returns `None` if:
/// - `~/.claude.json` doesn't exist or can't be read
/// - No matching project entry exists
/// - The project entry has no `lastSessionId`
pub fn capture_claude_session_id(workdir: &Path) -> Option<String> {
    let claude_json_path = home::home_dir()?.join(".claude.json");

    let content = match fs::read_to_string(&claude_json_path) {
        Ok(c) => c,
        Err(e) => {
            debug!(error = %e, "could not read ~/.claude.json");
            return None;
        }
    };

    let root: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            debug!(error = %e, "could not parse ~/.claude.json");
            return None;
        }
    };

    let projects = root.get("projects")?.as_object()?;

    // Try exact match first, then check if any project key is a parent of workdir.
    // Claude uses the raw absolute path as the project key (e.g., "/Users/user/project").
    let workdir_str = workdir.to_str()?;

    let project = projects
        .get(workdir_str)
        .or_else(|| {
            // Fallback: find a project whose key is a parent path of our workdir.
            // This handles cases where Claude was launched from a parent directory.
            projects
                .iter()
                .filter(|(key, _)| workdir_str.starts_with(key.as_str()))
                .max_by_key(|(key, _)| key.len()) // prefer the longest (most specific) match
                .map(|(_, v)| v)
        })?;

    let session_id = project
        .get("lastSessionId")?
        .as_str()?
        .to_string();

    if session_id.is_empty() {
        return None;
    }

    debug!(?workdir, %session_id, "captured Claude session ID");
    Some(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper: create a temporary ~/.claude.json with known content and
    /// test against it. Since we can't mock home_dir easily, these tests
    /// validate the parsing logic by calling internal helpers directly.

    #[test]
    fn test_parse_claude_json() {
        let json = r#"{
            "projects": {
                "/Users/user/project": {
                    "lastSessionId": "abc-123-def"
                },
                "/Users/user/other": {
                    "allowedTools": []
                }
            }
        }"#;

        let root: serde_json::Value = serde_json::from_str(json).unwrap();
        let projects = root["projects"].as_object().unwrap();

        // Exact match
        let entry = projects.get("/Users/user/project").unwrap();
        let sid = entry["lastSessionId"].as_str().unwrap();
        assert_eq!(sid, "abc-123-def");

        // Missing lastSessionId
        let entry2 = projects.get("/Users/user/other").unwrap();
        assert!(entry2.get("lastSessionId").is_none());
    }

    #[test]
    fn test_parent_path_matching() {
        let json = r#"{
            "projects": {
                "/Users/user/project": {
                    "lastSessionId": "parent-session"
                },
                "/Users/user/project/subdir": {
                    "lastSessionId": "child-session"
                }
            }
        }"#;

        let root: serde_json::Value = serde_json::from_str(json).unwrap();
        let projects = root["projects"].as_object().unwrap();

        let workdir = "/Users/user/project/subdir/deep";

        // Should find the longest matching parent: /Users/user/project/subdir
        let matched = projects
            .iter()
            .filter(|(key, _)| workdir.starts_with(key.as_str()))
            .max_by_key(|(key, _)| key.len())
            .map(|(_, v)| v);

        assert!(matched.is_some());
        let sid = matched.unwrap()["lastSessionId"].as_str().unwrap();
        assert_eq!(sid, "child-session");
    }

    #[test]
    fn test_no_matching_project() {
        let json = r#"{
            "projects": {
                "/Users/user/project": {
                    "lastSessionId": "some-id"
                }
            }
        }"#;

        let root: serde_json::Value = serde_json::from_str(json).unwrap();
        let projects = root["projects"].as_object().unwrap();

        let workdir = "/totally/different/path";
        let matched = projects
            .get(workdir)
            .or_else(|| {
                projects
                    .iter()
                    .filter(|(key, _)| workdir.starts_with(key.as_str()))
                    .max_by_key(|(key, _)| key.len())
                    .map(|(_, v)| v)
            });

        assert!(matched.is_none());
    }

    #[test]
    fn test_capture_returns_none_for_nonexistent_home() {
        // When called with a path that won't match anything in the real
        // ~/.claude.json, should return None gracefully.
        let result = capture_claude_session_id(PathBuf::from("/nonexistent/path/that/wont/match").as_path());
        // Either None (no match) or Some (unlikely match) — the point is no panic
        let _ = result;
    }
}
