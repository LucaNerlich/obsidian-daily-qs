//! Omarchy Quattro backend for Obsidian daily note todos.

use clap::{Parser, Subcommand};

use chrono::{Local, NaiveDate};

use std::io::{self, Write};

use obsidian_daily_qs::config::Vault;
use obsidian_daily_qs::status::Snapshot;
use obsidian_daily_qs::watch;
use obsidian_daily_qs::{
    add_todo, carry_over, open_in_obsidian, read_snapshot, toggle_todo, toggle_todo_in_file,
};

#[derive(Parser)]
#[command(
    name = "obsidian-daily-qs",
    version,
    about = "Backend for the Omarchy Obsidian Daily bar widget",
    long_about = "Reads and updates markdown checkbox todos in Obsidian daily \
                  notes for the Omarchy Quattro widget. Requires OBSIDIAN_VAULT_ROOT."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print one status snapshot as a single JSON line and exit
    Status {
        /// Calendar day `YYYY-MM-DD` (default: today)
        #[arg(long)]
        date: Option<String>,
    },
    /// Stream today's status snapshots as JSON lines when the note changes
    Watch,
    /// Add an open checkbox todo
    Add {
        /// Todo text (required, non-empty after trim)
        #[arg(long)]
        text: String,
        /// Calendar day `YYYY-MM-DD` (default: today)
        #[arg(long)]
        date: Option<String>,
    },
    /// Toggle a checkbox on the given 1-based source line
    Toggle {
        /// 1-based line number of the checkbox in the daily note
        #[arg(long)]
        line: usize,
        /// Todo text the caller saw at that line; refuse if it changed
        #[arg(long)]
        expect_text: Option<String>,
        /// Calendar day `YYYY-MM-DD` (default: today)
        #[arg(long)]
        date: Option<String>,
        /// Vault-relative file path to toggle in (e.g. `10-19_Personal/12-Health/Routines.md`); defaults to daily note
        #[arg(long)]
        file: Option<String>,
    },
    /// Copy yesterday's still-open todos into the target day
    CarryOver {
        /// Target day `YYYY-MM-DD` (default: today)
        #[arg(long)]
        date: Option<String>,
    },
    /// Open the daily note in Obsidian (`xdg-open obsidian://…`)
    Open {
        /// Calendar day `YYYY-MM-DD` (default: today)
        #[arg(long)]
        date: Option<String>,
    },
}

fn main() {
    match Cli::parse().command {
        Command::Status { date } => emit(run(read_snapshot, date)),
        Command::Watch => watch::watch(),
        Command::Add { text, date } => emit(run(|vault, d| add_todo(vault, d, &text), date)),
        Command::Toggle {
            line,
            expect_text,
            date,
            file,
        } => emit(match file {
            Some(f) => run(
                |vault, d| toggle_todo_in_file(vault, &f, line, expect_text.as_deref(), d),
                date,
            ),
            None => run(
                |vault, d| toggle_todo(vault, d, line, expect_text.as_deref()),
                date,
            ),
        }),
        Command::CarryOver { date } => emit(run(carry_over, date)),
        Command::Open { date } => emit(run(open_in_obsidian, date)),
    }
}

fn run<F>(f: F, date: Option<String>) -> Snapshot
where
    F: FnOnce(&Vault, NaiveDate) -> Result<Snapshot, obsidian_daily_qs::VaultError>,
{
    match Vault::from_env() {
        Ok(vault) => match parse_date(date) {
            Ok(d) => match f(&vault, d) {
                Ok(snap) => snap,
                Err(err) => Snapshot::error(err.to_string()),
            },
            Err(err) => Snapshot::error(err),
        },
        Err(err) => Snapshot::error(err.to_string()),
    }
}

fn parse_date(date: Option<String>) -> Result<NaiveDate, String> {
    match date {
        None => Ok(Local::now().date_naive()),
        Some(s) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
            .map_err(|_| format!("invalid --date {s:?}; expected YYYY-MM-DD")),
    }
}

fn emit(snap: Snapshot) {
    // Mirror watch::emit: a closed stdout (e.g. `status | head`) must exit
    // quietly instead of panicking on EPIPE.
    let line = serde_json::to_string(&snap).expect("snapshot serializes");
    let mut out = io::stdout().lock();
    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
        std::process::exit(0);
    }
}
