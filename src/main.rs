//! Omarchy Quattro backend for Obsidian daily note todos.

use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};

use obsidian_daily_qs::config::Vault;
use obsidian_daily_qs::status::{Snapshot, WeekSummary};
use obsidian_daily_qs::watch;
use obsidian_daily_qs::{
    add_todo_under, carry_over, delete_todo, edit_todo, open_in_obsidian, read_snapshot_filtered,
    set_indent, toggle_todo, undo_last, week_summary,
};

#[derive(Parser)]
#[command(
    name = "obsidian-daily-qs",
    version,
    about = "Backend for the Omarchy Obsidian Daily bar widget",
    long_about = "Reads and updates markdown checkbox todos in Obsidian daily \
                  notes for the Omarchy Quattro widget. Pass --vault or set \
                  OBSIDIAN_VAULT_ROOT."
)]
struct Cli {
    /// Absolute path to the Obsidian vault (overrides OBSIDIAN_VAULT_ROOT)
    #[arg(long, global = true)]
    vault: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print one status snapshot as a single JSON line and exit
    Status {
        #[arg(long)]
        date: Option<String>,
        /// Only include todos under this markdown heading (e.g. Todos)
        #[arg(long)]
        heading: Option<String>,
    },
    /// Stream today's status snapshots as JSON lines when the note changes
    Watch {
        #[arg(long)]
        heading: Option<String>,
    },
    /// Add an open checkbox todo
    Add {
        #[arg(long)]
        text: String,
        #[arg(long)]
        date: Option<String>,
        /// Nest under this 1-based todo line
        #[arg(long)]
        under_line: Option<usize>,
    },
    /// Toggle a checkbox on the given 1-based source line
    Toggle {
        #[arg(long)]
        line: usize,
        #[arg(long)]
        expect_text: Option<String>,
        #[arg(long)]
        date: Option<String>,
    },
    /// Rewrite the text of a todo on the given line
    Edit {
        #[arg(long)]
        line: usize,
        #[arg(long)]
        text: String,
        #[arg(long)]
        expect_text: Option<String>,
        #[arg(long)]
        date: Option<String>,
    },
    /// Delete a todo (optionally with nested children)
    Delete {
        #[arg(long)]
        line: usize,
        #[arg(long)]
        expect_text: Option<String>,
        #[arg(long, default_value_t = false)]
        with_children: bool,
        #[arg(long)]
        date: Option<String>,
    },
    /// Indent a todo one level
    Indent {
        #[arg(long)]
        line: usize,
        #[arg(long)]
        expect_text: Option<String>,
        #[arg(long)]
        date: Option<String>,
    },
    /// Outdent a todo one level
    Outdent {
        #[arg(long)]
        line: usize,
        #[arg(long)]
        expect_text: Option<String>,
        #[arg(long)]
        date: Option<String>,
    },
    /// Restore the previous note contents from the last mutation
    Undo,
    /// Print Mon–Sun open/done counts for the week containing `date`
    Week {
        #[arg(long)]
        date: Option<String>,
    },
    /// Move yesterday's still-open todos into the target day
    CarryOver {
        #[arg(long)]
        date: Option<String>,
    },
    /// Open the daily note in Obsidian (`xdg-open obsidian://…`)
    Open {
        #[arg(long)]
        date: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let vault_arg = cli.vault.clone();
    match cli.command {
        Command::Status { date, heading } => emit(run(
            vault_arg,
            |vault, d| read_snapshot_filtered(vault, d, heading.as_deref()),
            date,
        )),
        Command::Watch { heading } => watch::watch(vault_arg, heading),
        Command::Add {
            text,
            date,
            under_line,
        } => emit(run(
            vault_arg,
            |vault, d| add_todo_under(vault, d, &text, under_line),
            date,
        )),
        Command::Toggle {
            line,
            expect_text,
            date,
        } => emit(run(
            vault_arg,
            |vault, d| toggle_todo(vault, d, line, expect_text.as_deref()),
            date,
        )),
        Command::Edit {
            line,
            text,
            expect_text,
            date,
        } => emit(run(
            vault_arg,
            |vault, d| edit_todo(vault, d, line, expect_text.as_deref(), &text),
            date,
        )),
        Command::Delete {
            line,
            expect_text,
            with_children,
            date,
        } => emit(run(
            vault_arg,
            |vault, d| delete_todo(vault, d, line, expect_text.as_deref(), with_children),
            date,
        )),
        Command::Indent {
            line,
            expect_text,
            date,
        } => emit(run(
            vault_arg,
            |vault, d| set_indent(vault, d, line, expect_text.as_deref(), 1),
            date,
        )),
        Command::Outdent {
            line,
            expect_text,
            date,
        } => emit(run(
            vault_arg,
            |vault, d| set_indent(vault, d, line, expect_text.as_deref(), -1),
            date,
        )),
        Command::Undo => emit(match Vault::resolve(vault_arg) {
            Ok(vault) => match undo_last(&vault) {
                Ok(snap) => snap,
                Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
            },
            Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
        }),
        Command::Week { date } => {
            let out = match Vault::resolve(vault_arg) {
                Ok(vault) => match parse_date(date) {
                    Ok(d) => match week_summary(&vault, d) {
                        Ok(w) => w,
                        Err(err) => WeekSummary::error(err.to_string(), err.error_code()),
                    },
                    Err(err) => WeekSummary::error(err, "io"),
                },
                Err(err) => WeekSummary::error(err.to_string(), err.error_code()),
            };
            emit_json(&out);
        }
        Command::CarryOver { date } => emit(run(vault_arg, carry_over, date)),
        Command::Open { date } => emit(run(vault_arg, open_in_obsidian, date)),
    }
}

fn run<F>(vault_arg: Option<PathBuf>, f: F, date: Option<String>) -> Snapshot
where
    F: FnOnce(&Vault, NaiveDate) -> Result<Snapshot, obsidian_daily_qs::VaultError>,
{
    match Vault::resolve(vault_arg) {
        Ok(vault) => match parse_date(date) {
            Ok(d) => match f(&vault, d) {
                Ok(snap) => snap,
                Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
            },
            Err(err) => Snapshot::error_with_code(err, "io"),
        },
        Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
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
    emit_json(&snap);
}

fn emit_json<T: serde::Serialize>(value: &T) {
    let line = serde_json::to_string(value).expect("serializes");
    let mut out = io::stdout().lock();
    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
        std::process::exit(0);
    }
}
