//! File-based single-slot undo for the last note mutation.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::config::{Vault, VaultError};
use crate::status::Snapshot;
use crate::todos::read_snapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UndoRecord {
    vault: String,
    date: String,
    path: String,
    before: String,
}

fn undo_path() -> PathBuf {
    if let Ok(override_path) = std::env::var("OBSIDIAN_DAILY_QS_UNDO_PATH") {
        let trimmed = override_path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    cache.join("obsidian-daily-qs").join("last-undo.json")
}

pub fn record_before(
    vault: &Vault,
    date: NaiveDate,
    path: &Path,
    before: &str,
) -> Result<(), VaultError> {
    let record = UndoRecord {
        vault: vault.root().display().to_string(),
        date: date.format("%Y-%m-%d").to_string(),
        path: path.display().to_string(),
        before: before.to_string(),
    };
    let dest = undo_path();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            VaultError::Io(format!(
                "failed to create undo dir {}: {e}",
                parent.display()
            ))
        })?;
    }
    let json = serde_json::to_string_pretty(&record)
        .map_err(|e| VaultError::Io(format!("failed to serialize undo: {e}")))?;
    fs::write(&dest, json)
        .map_err(|e| VaultError::Io(format!("failed to write undo {}: {e}", dest.display())))
}

pub fn undo_last(vault: &Vault) -> Result<Snapshot, VaultError> {
    let dest = undo_path();
    if !dest.exists() {
        return Err(VaultError::Io("nothing to undo".into()));
    }
    let raw = fs::read_to_string(&dest)
        .map_err(|e| VaultError::Io(format!("failed to read undo {}: {e}", dest.display())))?;
    let record: UndoRecord = serde_json::from_str(&raw)
        .map_err(|e| VaultError::Io(format!("failed to parse undo: {e}")))?;
    let vault_s = vault.root().display().to_string();
    if record.vault != vault_s {
        return Err(VaultError::Io(
            "undo record is for a different vault; refusing to restore".into(),
        ));
    }
    let date = NaiveDate::parse_from_str(&record.date, "%Y-%m-%d")
        .map_err(|_| VaultError::Io(format!("invalid undo date {}", record.date)))?;
    let path = PathBuf::from(&record.path);
    crate::todos::write_atomic_public(vault.root(), &path, &record.before)?;
    let _ = fs::remove_file(&dest);
    read_snapshot(vault, date)
}
