//! Omarchy Quattro backend for Obsidian daily note todos.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};
use serde::Deserialize;

use obsidian_daily_qs::config::Vault;
use obsidian_daily_qs::status::{Snapshot, State, WeekSummary};
use obsidian_daily_qs::watch;
use obsidian_daily_qs::{
    add_todo_under, carry_over, defer_todo, delete_todo, edit_todo, open_in_obsidian,
    read_snapshot_filtered, set_indent, toggle_todo, undo_last, week_summary_filtered,
};

#[derive(Parser)]
#[command(
    name = "obsidian-daily-qs",
    version,
    about = "Backend for the Omarchy Obsidian Daily bar widget",
    long_about = "Reads and updates markdown checkbox todos in Obsidian daily \
                  notes for the Omarchy Quattro widget. Pass --vault or set \
                  OBSIDIAN_VAULT_ROOT, or hand the private fields over as one \
                  JSON object on stdin with --stdin (the widget path: nothing \
                  private in process arguments)."
)]
struct Cli {
    /// Absolute path to the Obsidian vault (overrides OBSIDIAN_VAULT_ROOT)
    #[arg(long, global = true)]
    vault: Option<PathBuf>,

    /// Optional archive folder pattern relative to the vault root
    /// (moment-style, e.g. dailies/_archive/YYYY) where old daily notes live
    #[arg(long, global = true)]
    archive_folder: Option<String>,

    /// Read private fields (`vault`, `archiveFolder`, `text`, `expectText`)
    /// as one JSON object from stdin instead of process arguments
    #[arg(long, global = true)]
    stdin: bool,

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
        #[arg(long)]
        date: Option<String>,
    },
    /// Delete a todo (optionally with nested children)
    Delete {
        #[arg(long)]
        line: usize,
        #[arg(long, conflicts_with = "stdin")]
        expect_text: Option<String>,
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
        #[arg(long)]
        date: Option<String>,
    },
    /// Outdent a todo one level
    Outdent {
        #[arg(long)]
        line: usize,
        #[arg(long, conflicts_with = "stdin")]
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
        /// Only count todos under this markdown heading (e.g. Todos)
        #[arg(long)]
        heading: Option<String>,
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

/// Private fields the widget sends on stdin so they never appear in argv.
///
/// `vault` and `archiveFolder` travel here too (#102): the widget's children
/// are long-lived (a watch runs for the whole session) and `/proc/<pid>/cmdline`
/// is readable by any local user on systems without `hidepid`.
///
/// `deny_unknown_fields` is load-bearing: an absent `expectText` means "no
/// stale-view check was requested", so a typo'd or snake_cased key would
/// otherwise be dropped silently and the mutation would run against a stale
/// view instead of erroring.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StdinFields {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    expect_text: Option<String>,
    #[serde(default)]
    vault: Option<String>,
    #[serde(default)]
    archive_folder: Option<String>,
}

impl StdinFields {
    /// Vault root from the payload; empty means "not sent".
    fn vault_path(&self) -> Option<PathBuf> {
        empty_to_none(self.vault.clone()).map(PathBuf::from)
    }

    /// Archive pattern from the payload; empty means "not sent".
    fn archive(&self) -> Option<String> {
        empty_to_none(self.archive_folder.clone())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Read the private channel first, once. Every widget launch carries the
    // vault here, and a malformed payload must fail the command rather than
    // silently fall back to argv/env: a "no vault" fallback would read or
    // mutate the wrong vault.
    let stdin_fields = if cli.stdin {
        read_stdin_fields()
    } else {
        Ok(StdinFields::default())
    };
    if let Err(err) = &stdin_fields {
        return command_error(&cli.command, err.clone());
    }
    let stdin_fields = stdin_fields.expect("checked above");
    // `Some` exactly when --stdin asked for the payload, so the resolve_*
    // helpers keep treating "no stdin" as "argv only".
    let fields = cli.stdin.then_some(&stdin_fields);
    if let Some(fields) = fields {
        let (takes_text, takes_expect) = command_fields(&cli.command);
        if let Err(err) = reject_unexpected(fields, takes_text, takes_expect) {
            return command_error(&cli.command, err);
        }
    }
    // Precedence: an explicit --vault wins over the payload, which wins over
    // OBSIDIAN_VAULT_ROOT inside Vault::resolve.
    let vault_arg = cli.vault.clone().or_else(|| stdin_fields.vault_path());
    let archive_arg = cli
        .archive_folder
        .clone()
        .or_else(|| stdin_fields.archive());
    match cli.command {
        Command::Status { date, heading } => emit(run(
            vault_arg,
            archive_arg,
            |vault, d| read_snapshot_filtered(vault, d, heading.as_deref()),
            date,
        )),
        Command::Watch { heading } => {
            watch::watch(vault_arg, archive_arg, heading);
            // The restart heuristic watches the child, not its status; a
            // long-running watch has no meaningful exit code to report.
            ExitCode::SUCCESS
        }
        Command::Add {
            text,
            date,
            under_line,
            heading,
        } => emit(match resolve_text(text, fields) {
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
            date,
        } => emit(match resolve_expect_text(expect_text, fields) {
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
            date,
        } => emit(match resolve_edit_fields(text, expect_text, fields) {
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
            with_children,
            date,
        } => emit(match resolve_expect_text(expect_text, fields) {
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
            with_children,
            date,
            heading,
        } => emit(match resolve_expect_text(expect_text, fields) {
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
            date,
        } => emit(match resolve_expect_text(expect_text, fields) {
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
            date,
        } => emit(match resolve_expect_text(expect_text, fields) {
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
        Command::Week { date, heading } => {
            let out = match Vault::resolve(vault_arg, archive_arg) {
                Ok(vault) => match parse_date(date) {
                    Ok(d) => match week_summary_filtered(&vault, d, heading.as_deref()) {
                        Ok(w) => w,
                        Err(err) => WeekSummary::error(err.to_string(), err.error_code()),
                    },
                    Err(err) => WeekSummary::error(err, "io"),
                },
                Err(err) => WeekSummary::error(err.to_string(), err.error_code()),
            };
            emit_json(&out, matches!(out.state, State::Error))
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
    // Parse to a value first: serde_derive also accepts a positional (array)
    // form for a struct, which `deny_unknown_fields` does not police — an array
    // body would decode positionally and a mistyped expectation would be
    // silently dropped, exactly the bug this guard exists to prevent.
    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|_| stdin_shape_error(0, 0))?;
    if !value.is_object() {
        return Err(stdin_shape_error(0, 0));
    }
    // Never format serde's error verbatim: its type errors quote the offending
    // value, which can be the user's private todo text, and this message is
    // surfaced by the widget.
    serde_json::from_value(value).map_err(|err| stdin_shape_error(err.line(), err.column()))
}

fn stdin_shape_error(line: usize, column: usize) -> String {
    let where_ = if line > 0 {
        format!(" (line {line}, column {column})")
    } else {
        String::new()
    };
    format!(
        "invalid stdin JSON: expected an object with \
         `vault`/`archiveFolder`/`text`/`expectText`{where_}"
    )
}

/// Which todo fields each command accepts from the payload. The transport
/// fields (`vault`, `archiveFolder`) are accepted everywhere; a payload that
/// names a field the command does not use is rejected by
/// [`reject_unexpected`] rather than silently dropped.
fn command_fields(command: &Command) -> (bool, bool) {
    match command {
        Command::Add { .. } => (true, false),
        Command::Edit { .. } => (true, true),
        Command::Toggle { .. }
        | Command::Delete { .. }
        | Command::Defer { .. }
        | Command::Indent { .. }
        | Command::Outdent { .. } => (false, true),
        Command::Status { .. }
        | Command::Watch { .. }
        | Command::Week { .. }
        | Command::Undo
        | Command::CarryOver { .. }
        | Command::Open { .. } => (false, false),
    }
}

/// Reject a non-empty field the command does not take. Silently ignoring it
/// would let a producer that names the expected text `text` skip the
/// stale-view guard and mutate the wrong line with a success result (#53).
fn reject_unexpected(
    fields: &StdinFields,
    takes_text: bool,
    takes_expect: bool,
) -> Result<(), String> {
    if !takes_text && fields.text.as_deref().is_some_and(|t| !t.trim().is_empty()) {
        return Err(if takes_expect {
            "unexpected `text` on stdin: this command takes `expectText`".into()
        } else {
            "unexpected `text` on stdin: this command takes no todo text".into()
        });
    }
    if !takes_expect
        && fields
            .expect_text
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
    {
        return Err(if takes_text {
            "unexpected `expectText` on stdin: this command takes only `text`".into()
        } else {
            "unexpected `expectText` on stdin: this command takes no expectation".into()
        });
    }
    Ok(())
}

/// The failure shape each command reports: `week` answers with a
/// [`WeekSummary`], everything else with a [`Snapshot`].
fn command_error(command: &Command, message: String) -> ExitCode {
    match command {
        Command::Week { .. } => {
            let out = WeekSummary::error(message, "io");
            emit_json(&out, true)
        }
        _ => emit(Snapshot::error_with_code(message, "io")),
    }
}

fn resolve_text(text: Option<String>, fields: Option<&StdinFields>) -> Result<String, String> {
    match fields {
        Some(fields) => non_empty_field(fields.text.clone(), "text"),
        None => non_empty_field(text, "--text"),
    }
}

fn resolve_expect_text(
    expect_text: Option<String>,
    fields: Option<&StdinFields>,
) -> Result<Option<String>, String> {
    match fields {
        Some(fields) => Ok(empty_to_none(fields.expect_text.clone())),
        None => Ok(empty_to_none(expect_text)),
    }
}

fn resolve_edit_fields(
    text: Option<String>,
    expect_text: Option<String>,
    fields: Option<&StdinFields>,
) -> Result<(String, Option<String>), String> {
    match fields {
        Some(fields) => Ok((
            non_empty_field(fields.text.clone(), "text")?,
            empty_to_none(fields.expect_text.clone()),
        )),
        None => Ok((non_empty_field(text, "--text")?, empty_to_none(expect_text))),
    }
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

fn emit(snap: Snapshot) -> ExitCode {
    let failed = matches!(snap.state, State::Error);
    emit_json(&snap, failed)
}

/// Write one JSON line to stdout and report the outcome as a process status.
///
/// A one-shot command that produced an error snapshot, or whose output did not
/// reach the caller, must not exit 0: `cmd || fallback`, CI steps and shell
/// wrappers would otherwise treat a failed mutation as success.
fn emit_json<T: serde::Serialize>(value: &T, failed: bool) -> ExitCode {
    let line = serde_json::to_string(value).expect("serializes");
    let mut out = io::stdout().lock();
    let written = writeln!(out, "{line}").and_then(|()| out.flush());
    if let Err(err) = written {
        // Never panic on a closed stdout (see #12). A reader that went away
        // (`cmd | head`) consumed what it asked for, so that is not a failure;
        // any other write error means the caller did not get the output.
        return if err.kind() == io::ErrorKind::BrokenPipe {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
