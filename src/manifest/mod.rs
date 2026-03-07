//! Session manifest for durable session tracking and archival.
//!
//! The manifest is a global JSON file at `~/.local/state/workmux/manifest.json`
//! that records every workmux session (worktree and general) with enough
//! context to survive restarts and support archival + Claude session resume.
//!
//! This is an **additive layer** on top of the existing agent state system.
//! The upstream workmux code paths are not modified — the manifest is read
//! and written only by our additions.

pub mod claude;
pub mod store;
pub mod types;

// Re-exports used by integration points in later phases (create, close, cleanup, etc.)
#[allow(unused_imports)]
pub use store::ManifestStore;
#[allow(unused_imports)]
pub use types::{Lifecycle, Manifest, ManifestEntry, SessionType, manifest_key, unix_now};
