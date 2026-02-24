use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::multiplexer::{create_backend, detect_backend};
use crate::{config, workflow};
use crate::workflow::{SetupOptions, WorkflowContext};

pub fn run(
    name: &str,
    dir: Option<PathBuf>,
    session: bool,
    agent: Option<&str>,
    no_pane_cmds: bool,
    background: bool,
) -> Result<()> {
    let config = config::Config::load(agent)?;
    let mux = create_backend(detect_backend());

    let working_dir = match dir {
        Some(d) => d,
        None => std::env::current_dir().context("Failed to get current directory")?,
    };

    if !working_dir.exists() {
        anyhow::bail!(
            "Directory '{}' does not exist",
            working_dir.display()
        );
    }

    let mode = if session {
        config::MuxMode::Session
    } else {
        config.mode()
    };

    // run_hooks=false: skip post_create hooks (repo-specific, not applicable for general sessions)
    // run_file_ops=false: skip file copy/symlink operations (also repo-specific)
    let options = SetupOptions {
        run_hooks: false,
        run_file_ops: false,
        run_pane_commands: !no_pane_cmds,
        prompt_file_path: None,
        focus_window: !background,
        working_dir: None,
        config_root: None,
        open_if_exists: false,
        mode,
    };

    let context = WorkflowContext::new_general(working_dir.clone(), config, mux)?;
    context.ensure_mux_running()?;

    let result = workflow::create_general_session(name, &working_dir, &context, options, agent)
        .context("Failed to create general session")?;

    let kind = if mode == config::MuxMode::Session { "session" } else { "window" };
    println!(
        "✓ Created {} '{}{}' in '{}'",
        kind,
        context.prefix,
        name,
        result.worktree_path.display()
    );

    Ok(())
}
