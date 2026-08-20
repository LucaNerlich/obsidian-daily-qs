//! Obsidian daily-notes backend for the Omarchy Quattro bar widget.

pub mod config;
pub mod format;
pub mod status;
pub mod todos;
pub mod watch;

pub use config::{DailyNotesConfig, Vault, VaultError};
pub use status::{Snapshot, State, TodoItem};
pub use todos::{add_todo, ensure_note, read_snapshot, toggle_todo};
