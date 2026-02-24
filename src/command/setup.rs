use anyhow::{Context, Result};
use console::style;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use crate::agent_setup::{self, StatusCheck};

pub fn run() -> Result<()> {
    if !io::stdin().is_terminal() {
        anyhow::bail!("workmux setup requires an interactive terminal");
    }

    let checks = agent_setup::check_all();

    if checks.is_empty() {
        println!(
            "No agents detected. Install an agent CLI (Claude Code, OpenCode) to get started."
        );
        return Ok(());
    }

    println!();
    let mut any_needed = false;

    for check in &checks {
        let status_str = match &check.status {
            StatusCheck::Installed => format!("{}", style("configured").green()),
            StatusCheck::NotInstalled => {
                any_needed = true;
                format!("{}", style("not configured").yellow())
            }
            StatusCheck::Error(e) => {
                any_needed = true;
                format!("{} ({})", style("error").red(), e)
            }
        };

        println!(
            "  {} {} ({}): {}",
            style("•").dim(),
            check.agent.name(),
            style(check.reason).dim(),
            status_str
        );
    }
    println!();

    if !any_needed {
        println!(
            "{}",
            style("All agents have status tracking configured.").green()
        );
        return Ok(());
    }

    let needs_setup: Vec<_> = checks
        .iter()
        .filter(|c| matches!(c.status, StatusCheck::NotInstalled | StatusCheck::Error(_)))
        .collect();

    agent_setup::print_description("");
    println!();

    if confirm_install()? {
        let mut any_failed = false;
        for check in &needs_setup {
            match agent_setup::install(check.agent) {
                Ok(msg) => println!("  {} {}", style("✓").green(), msg),
                Err(e) => {
                    println!("  {} {}: {}", style("✗").red(), check.agent.name(), e);
                    any_failed = true;
                }
            }
        }
        println!();
        if any_failed {
            anyhow::bail!("Some installations failed");
        }
    } else {
        println!();
    }

    // Offer to install the dashboard keybinding (Ctrl-b Ctrl-c)
    offer_dashboard_keybinding()?;

    Ok(())
}

const DASHBOARD_KEYBINDING: &str =
    r#"bind-key C-c display-popup -E -w 90% -h 90% "workmux dashboard""#;

fn offer_dashboard_keybinding() -> Result<()> {
    let tmux_conf = tmux_conf_path();

    // Check if already installed
    if tmux_conf.exists() {
        let content = std::fs::read_to_string(&tmux_conf)
            .context("Failed to read ~/.tmux.conf")?;
        if content.contains(DASHBOARD_KEYBINDING) {
            println!(
                "  {} Dashboard keybinding (Ctrl-b Ctrl-c) already installed",
                style("✓").green()
            );
            return Ok(());
        }
    }

    let prompt = format!(
        "  Add dashboard keybinding (Ctrl-b Ctrl-c) to {}? {}{}{} ",
        tmux_conf.display(),
        style("[").bold().cyan(),
        style("Y/n").bold(),
        style("]").bold().cyan(),
    );

    print!("{}", prompt);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();

    match answer.as_str() {
        "" | "y" | "yes" => {
            // Append keybinding to ~/.tmux.conf
            let line = format!("\n# workmux: open dashboard popup\n{}\n", DASHBOARD_KEYBINDING);
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&tmux_conf)
                .and_then(|mut f| {
                    use io::Write;
                    f.write_all(line.as_bytes())
                })
                .with_context(|| {
                    format!("Failed to write to {}", tmux_conf.display())
                })?;
            println!(
                "  {} Dashboard keybinding added to {}",
                style("✓").green(),
                tmux_conf.display()
            );
            println!(
                "    {}",
                style("Reload tmux config with: tmux source ~/.tmux.conf").dim()
            );
        }
        _ => {
            println!("  {} Skipped dashboard keybinding", style("·").dim());
        }
    }

    Ok(())
}

fn tmux_conf_path() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".tmux.conf"))
        .unwrap_or_else(|_| PathBuf::from("~/.tmux.conf"))
}

fn confirm_install() -> Result<bool> {
    let prompt = format!(
        "  Install status tracking hooks? {}{}{} ",
        style("[").bold().cyan(),
        style("Y/n").bold(),
        style("]").bold().cyan(),
    );

    loop {
        print!("{}", prompt);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let answer = input.trim().to_lowercase();

        match answer.as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("    {}", style("Please enter y or n").dim()),
        }
    }
}
