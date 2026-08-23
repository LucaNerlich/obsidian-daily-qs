//! Obsidian daily-notes backend for the Omarchy Quattro bar widget.

pub mod config;
pub mod dataview;
pub mod format;
pub mod open;
pub mod status;
pub mod todos;
pub mod watch;

pub use config::{DailyNotesConfig, Vault, VaultError};
pub use status::{Snapshot, State, TodoItem};
pub use todos::{add_todo, carry_over, ensure_note, open_in_obsidian, read_snapshot, toggle_todo};
