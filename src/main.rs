//! Omarchy Quattro backend for Obsidian daily note todos.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};
use serde::Deserialize;

use obsidian_daily_qs::config::Vault;
use obsidian_daily_qs::status::{Snapshot, WeekSummary};
use obsidian_daily_qs::watch;
use obsidian_daily_qs::{
    add_todo_under, carry_over, defer_todo, delete_todo, edit_todo, open_in_obsidian,
    read_snapshot_filtered, set_indent, toggle_todo, undo_last, week_summary,
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

    /// Optional archive folder pattern relative to the vault root
    /// (moment-style, e.g. dailies/_archive/YYYY) where old daily notes live
    #[arg(long, global = true)]
    archive_folder: Option<String>,

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
        /// Todo text (visible in process arguments; prefer --stdin from the widget)
        #[arg(long, conflicts_with = "stdin")]
        text: Option<String>,
        /// Read `{"text":"..."}` from stdin instead of --text
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        date: Option<String>,
        /// Nest under this 1-based todo line
        #[arg(long)]
        under_line: Option<usize>,
        /// Insert under this markdown heading (e.g. Inbox); falls back to
        /// the default Tasks/Todos placement when missing
        #[arg(long)]
        heading: Option<String>,
    },
    /// Toggle a checkbox on the given 1-based source line
    Toggle {
        #[arg(long)]
        line: usize,
        #[arg(long, conflicts_with = "stdin")]
        expect_text: Option<String>,
        /// Read `{"expectText":"..."}` from stdin instead of --expect-text
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        date: Option<String>,
    },
    /// Rewrite the text of a todo on the given line
    Edit {
        #[arg(long)]
        line: usize,
        /// New todo text (visible in process arguments; prefer --stdin)
        #[arg(long, conflicts_with = "stdin")]
        text: Option<String>,
        #[arg(long, conflicts_with = "stdin")]
        expect_text: Option<String>,
        /// Read `{"text":"...","expectText":"..."}` from stdin
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        date: Option<String>,
    },
    /// Delete a todo (optionally with nested children)
    Delete {
        #[arg(long)]
        line: usize,
        #[arg(long, conflicts_with = "stdin")]
        expect_text: Option<String>,
        /// Read `{"expectText":"..."}` from stdin instead of --expect-text
        #[arg(long)]
        stdin: bool,
        #[arg(long, default_value_t = false)]
        with_children: bool,
        #[arg(long)]
        date: Option<String>,
    },
    /// Move a todo to the next day's note (creates that note if missing)
    Defer {
        #[arg(long)]
        line: usize,
        #[arg(long, conflicts_with = "stdin")]
        expect_text: Option<String>,
        /// Read `{"expectText":"..."}` from stdin instead of --expect-text
        #[arg(long)]
        stdin: bool,
        #[arg(long, default_value_t = false)]
        with_children: bool,
        #[arg(long)]
        date: Option<String>,
        /// Insert under this markdown heading in the destination note
        #[arg(long)]
        heading: Option<String>,
    },
    /// Indent a todo one level
    Indent {
        #[arg(long)]
        line: usize,
        #[arg(long, conflicts_with = "stdin")]
        expect_text: Option<String>,
        /// Read `{"expectText":"..."}` from stdin instead of --expect-text
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        date: Option<String>,
    },
    /// Outdent a todo one level
    Outdent {
        #[arg(long)]
        line: usize,
        #[arg(long, conflicts_with = "stdin")]
        expect_text: Option<String>,
        /// Read `{"expectText":"..."}` from stdin instead of --expect-text
        #[arg(long)]
        stdin: bool,
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
    /// Move the most recent previous daily note's still-open todos into the target day
    CarryOver {
        #[arg(long)]
        date: Option<String>,
        /// Insert carried todos under this markdown heading
        #[arg(long)]
        heading: Option<String>,
    },
    /// Create the daily note when missing, then open it in Obsidian
    Open {
        #[arg(long)]
        date: Option<String>,
    },
}

/// Sensitive fields the widget sends on stdin so they never appear in argv.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StdinFields {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    expect_text: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    let vault_arg = cli.vault.clone();
    let archive_arg = cli.archive_folder.clone();
    match cli.command {
        Command::Status { date, heading } => emit(run(
            vault_arg,
            archive_arg,
            |vault, d| read_snapshot_filtered(vault, d, heading.as_deref()),
            date,
        )),
        Command::Watch { heading } => {
            watch::watch(cli.vault.clone(), cli.archive_folder.clone(), heading)
        }
        Command::Add {
            text,
            stdin,
            date,
            under_line,
            heading,
        } => emit(match resolve_text(text, stdin) {
            Ok(text) => run(
                vault_arg,
                archive_arg,
                |vault, d| add_todo_under(vault, d, &text, under_line, heading.as_deref()),
                date,
            ),
            Err(err) => Snapshot::error_with_code(err, "io"),
        }),
        Command::Toggle {
            line,
            expect_text,
            stdin,
            date,
        } => emit(match resolve_expect_text(expect_text, stdin) {
            Ok(expect_text) => run(
                vault_arg,
                archive_arg,
                |vault, d| toggle_todo(vault, d, line, expect_text.as_deref()),
                date,
            ),
            Err(err) => Snapshot::error_with_code(err, "io"),
        }),
        Command::Edit {
            line,
            text,
            expect_text,
            stdin,
            date,
        } => emit(match resolve_edit_fields(text, expect_text, stdin) {
            Ok((text, expect_text)) => run(
                vault_arg,
                archive_arg,
                |vault, d| edit_todo(vault, d, line, expect_text.as_deref(), &text),
                date,
            ),
            Err(err) => Snapshot::error_with_code(err, "io"),
        }),
        Command::Delete {
            line,
            expect_text,
            stdin,
            with_children,
            date,
        } => emit(match resolve_expect_text(expect_text, stdin) {
            Ok(expect_text) => run(
                vault_arg,
                archive_arg,
                |vault, d| delete_todo(vault, d, line, expect_text.as_deref(), with_children),
                date,
            ),
            Err(err) => Snapshot::error_with_code(err, "io"),
        }),
        Command::Defer {
            line,
            expect_text,
            stdin,
            with_children,
            date,
            heading,
        } => emit(match resolve_expect_text(expect_text, stdin) {
            Ok(expect_text) => run(
                vault_arg,
                archive_arg,
                |vault, d| {
                    defer_todo(
                        vault,
                        d,
                        line,
                        expect_text.as_deref(),
                        with_children,
                        heading.as_deref(),
                    )
                },
                date,
            ),
            Err(err) => Snapshot::error_with_code(err, "io"),
        }),
        Command::Indent {
            line,
            expect_text,
            stdin,
            date,
        } => emit(match resolve_expect_text(expect_text, stdin) {
            Ok(expect_text) => run(
                vault_arg,
                archive_arg,
                |vault, d| set_indent(vault, d, line, expect_text.as_deref(), 1),
                date,
            ),
            Err(err) => Snapshot::error_with_code(err, "io"),
        }),
        Command::Outdent {
            line,
            expect_text,
            stdin,
            date,
        } => emit(match resolve_expect_text(expect_text, stdin) {
            Ok(expect_text) => run(
                vault_arg,
                archive_arg,
                |vault, d| set_indent(vault, d, line, expect_text.as_deref(), -1),
                date,
            ),
            Err(err) => Snapshot::error_with_code(err, "io"),
        }),
        Command::Undo => emit(match Vault::resolve(vault_arg, archive_arg) {
            Ok(vault) => match undo_last(&vault) {
                Ok(snap) => snap,
                Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
            },
            Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
        }),
        Command::Week { date } => {
            let out = match Vault::resolve(vault_arg, archive_arg) {
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
        Command::CarryOver { date, heading } => emit(run(
            vault_arg,
            archive_arg,
            |vault, d| carry_over(vault, d, heading.as_deref()),
            date,
        )),
        Command::Open { date } => emit(run(vault_arg, archive_arg, open_in_obsidian, date)),
    }
}

fn read_stdin_fields() -> Result<StdinFields, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|err| format!("failed to read stdin: {err}"))?;
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        return Err("expected JSON fields on stdin".into());
    }
    serde_json::from_str(trimmed).map_err(|err| format!("invalid stdin JSON: {err}"))
}

fn resolve_text(text: Option<String>, stdin: bool) -> Result<String, String> {
    if stdin {
        let fields = read_stdin_fields()?;
        return non_empty_field(fields.text, "text");
    }
    non_empty_field(text, "--text")
}

fn resolve_expect_text(expect_text: Option<String>, stdin: bool) -> Result<Option<String>, String> {
    if !stdin {
        return Ok(empty_to_none(expect_text));
    }
    let fields = read_stdin_fields()?;
    Ok(empty_to_none(fields.expect_text))
}

fn resolve_edit_fields(
    text: Option<String>,
    expect_text: Option<String>,
    stdin: bool,
) -> Result<(String, Option<String>), String> {
    if stdin {
        let fields = read_stdin_fields()?;
        return Ok((
            non_empty_field(fields.text, "text")?,
            empty_to_none(fields.expect_text),
        ));
    }
    Ok((non_empty_field(text, "--text")?, empty_to_none(expect_text)))
}

fn non_empty_field(value: Option<String>, name: &str) -> Result<String, String> {
    match value.map(|s| s.trim().to_string()) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(format!("missing {name}")),
    }
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn run<F>(
    vault_arg: Option<PathBuf>,
    archive_arg: Option<String>,
    f: F,
    date: Option<String>,
) -> Snapshot
where
    F: FnOnce(&Vault, NaiveDate) -> Result<Snapshot, obsidian_daily_qs::VaultError>,
{
    match Vault::resolve(vault_arg, archive_arg) {
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
