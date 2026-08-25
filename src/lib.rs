//! Obsidian daily-notes backend for the Omarchy Quattro bar widget.

pub mod config;
pub mod format;
pub mod open;
pub mod status;
pub mod todos;
pub mod undo;
pub mod watch;

pub use config::{DailyNotesConfig, Vault, VaultError};
pub use status::{DaySummary, Snapshot, State, TodoItem, WeekSummary};
pub use todos::{
    add_todo, add_todo_under, carry_over, delete_todo, edit_todo, ensure_note, open_in_obsidian,
    read_snapshot, read_snapshot_filtered, set_indent, toggle_todo, week_summary,
};
pub use undo::undo_last;
