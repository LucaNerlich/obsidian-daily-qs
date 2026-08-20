//! Omarchy Quattro backend for Obsidian daily note todos.

use clap::{Parser, Subcommand};

use obsidian_daily_qs::config::Vault;
use obsidian_daily_qs::status::Snapshot;
use obsidian_daily_qs::watch::{self, current_snapshot};
use obsidian_daily_qs::{add_todo, toggle_todo};

#[derive(Parser)]
#[command(
    name = "obsidian-daily-qs",
    version,
    about = "Backend for the Omarchy Obsidian Daily bar widget",
    long_about = "Reads and updates markdown checkbox todos in today's Obsidian \
                  daily note for the Omarchy Quattro widget. Requires \
                  OBSIDIAN_VAULT_ROOT."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print one status snapshot as a single JSON line and exit
    Status,
    /// Stream status snapshots as JSON lines when the daily note changes
    Watch,
    /// Add an open checkbox todo to today's daily note
    Add {
        /// Todo text (required, non-empty after trim)
        #[arg(long)]
        text: String,
    },
    /// Toggle a checkbox on the given 1-based source line
    Toggle {
        /// 1-based line number of the checkbox in the daily note
        #[arg(long)]
        line: usize,
    },
}

fn main() {
    match Cli::parse().command {
        Command::Status => emit(current_snapshot()),
        Command::Watch => watch::watch(),
        Command::Add { text } => emit(run_mut(|vault, date| add_todo(vault, date, &text))),
        Command::Toggle { line } => emit(run_mut(|vault, date| toggle_todo(vault, date, line))),
    }
}

fn run_mut<F>(f: F) -> Snapshot
where
    F: FnOnce(&Vault, chrono::NaiveDate) -> Result<Snapshot, obsidian_daily_qs::VaultError>,
{
    match Vault::from_env() {
        Ok(vault) => {
            let date = chrono::Local::now().date_naive();
            match f(&vault, date) {
                Ok(snap) => snap,
                Err(err) => Snapshot::error(err.to_string()),
            }
        }
        Err(err) => Snapshot::error(err.to_string()),
    }
}

fn emit(snap: Snapshot) {
    println!(
        "{}",
        serde_json::to_string(&snap).expect("snapshot serializes")
    );
}
