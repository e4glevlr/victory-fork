//! Repository maintenance. Run with `cargo xtask <command>`.

mod policy;
mod viz;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Enforce the asset policy: the repository holds only small text files
    /// we wrote. Checks tracked and untracked-but-not-ignored files.
    Policy {
        /// Check only what is staged for commit (used by the pre-commit hook).
        #[arg(long)]
        staged: bool,
    },
    /// Generate the SVG architecture and telemetry visualization for README.md.
    Viz {
        /// Output path relative to the workspace root (default: docs/repo-visualization.svg).
        #[arg(long, default_value = viz::DEFAULT_OUTPUT)]
        output: PathBuf,
        /// Account for the commit currently being created (used by pre-commit).
        #[arg(long)]
        pre_commit: bool,
        /// Stage the updated SVG file with `git add` after writing it.
        #[arg(long)]
        stage: bool,
    },
    /// Point git at .githooks/ so every commit updates the README visualization
    /// and runs the policy check.
    InstallHooks,
}

fn main() -> Result<()> {
    let root = workspace_root();
    match Cli::parse().command {
        Cmd::Policy { staged } => policy::run(&root, staged),
        Cmd::Viz {
            output,
            pre_commit,
            stage,
        } => {
            let full = if output.is_absolute() {
                output
            } else {
                root.join(output)
            };
            viz::run(&root, &full, pre_commit, stage)
        }
        Cmd::InstallHooks => {
            let status = Command::new("git")
                .args(["config", "core.hooksPath", ".githooks"])
                .current_dir(&root)
                .status()
                .context("running git")?;
            anyhow::ensure!(
                status.success(),
                "git config failed (is this a git repository?)"
            );
            println!(
                "hooks installed: commits now refresh {} and run `cargo xtask policy --staged`",
                viz::DEFAULT_OUTPUT
            );
            Ok(())
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .into()
}
