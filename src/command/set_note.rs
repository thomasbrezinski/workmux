use anyhow::Result;

use crate::multiplexer::{create_backend, detect_backend};

/// Set (or clear, when empty) the linked Obsidian note name for the current
/// tmux window. Stored in the `@wmx_note` window option and surfaced by the
/// sidebar `{note}` token. Typically invoked by the `obsidian-note` skill when
/// it links a note to the session, but also usable by hand.
pub fn run(name: &str) -> Result<()> {
    let mux = create_backend(detect_backend());
    let Some(pane_id) = mux.current_pane_id() else {
        // Not inside a multiplexer pane; nothing to tag.
        return Ok(());
    };
    mux.set_note(&pane_id, name)?;
    Ok(())
}
