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
    write_undo_file(&dest, &json)
        .map_err(|e| VaultError::Io(format!("failed to write undo {}: {e}", dest.display())))?;

    // The undo file holds the previous note contents (the user's daily todos).
    // Restrict it to the owner so another local user cannot read it through a
    // world-traversable cache directory.
    if let Err(e) = restrict_undo_permissions(&dest) {
        let _ = fs::remove_file(&dest);
        return Err(VaultError::Io(format!(
            "failed to secure undo {}: {e}",
            dest.display()
        )));
    }
    Ok(())
}

/// Create the undo file owner-only (0600) on Unix. On other platforms the
/// `fs::OpenOptions::mode` call is unavailable and the default umask applies.
#[cfg(unix)]
fn write_undo_file(dest: &Path, json: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(dest)
        .and_then(|mut f| f.write_all(json.as_bytes()))
}

#[cfg(not(unix))]
fn write_undo_file(dest: &Path, json: &str) -> std::io::Result<()> {
    fs::write(dest, json)
}

/// Lock down the undo file's permissions to owner-only on Unix. `write_undo_file`
/// creates the file 0600, but an existing undo file from a previous version (or a
/// reused path) may be 0644 — `set_permissions` ensures the restriction regardless
/// of how the file was created.
#[cfg(unix)]
fn restrict_undo_permissions(dest: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dest, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_undo_permissions(_dest: &Path) -> std::io::Result<()> {
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmp_undo_path() -> PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "obsidian-daily-qs-undo-{}-{n}.json",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    #[test]
    fn undo_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let undo_file = tmp_undo_path();
        let _ = fs::remove_file(&undo_file);
        // SAFETY: tests are single-threaded; no other thread reads env vars.
        unsafe {
            std::env::set_var("OBSIDIAN_DAILY_QS_UNDO_PATH", &undo_file);
        }

        let vault_root = std::env::temp_dir().join(format!(
            "obsidian-daily-qs-undo-vault-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&vault_root);
        fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault {
            root: vault_root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let note_path = vault_root.join("2026-08-20.md");

        record_before(&vault, date, &note_path, "- [ ] secret\n").unwrap();

        let mode = fs::metadata(&undo_file).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "undo file should be owner-only (0600), got {:04o}",
            mode & 0o777
        );

        let _ = fs::remove_file(&undo_file);
        let _ = fs::remove_dir_all(&vault_root);
        // SAFETY: tests are single threaded.
        unsafe {
            std::env::remove_var("OBSIDIAN_DAILY_QS_UNDO_PATH");
        }
    }

    #[cfg(unix)]
    #[test]
    fn restricts_existing_world_readable_undo_file() {
        use std::os::unix::fs::PermissionsExt;
        let undo_file = tmp_undo_path();
        let _ = fs::remove_file(&undo_file);
        // Simulate a pre-existing undo file from a previous version that was
        // created world-readable (0644).
        fs::write(&undo_file, "{}").unwrap();
        fs::set_permissions(&undo_file, fs::Permissions::from_mode(0o644)).unwrap();

        // SAFETY: tests are single threaded.
        unsafe {
            std::env::set_var("OBSIDIAN_DAILY_QS_UNDO_PATH", &undo_file);
        }

        let vault_root = std::env::temp_dir().join(format!(
            "obsidian-daily-qs-undo-vault-existing-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&vault_root);
        fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault {
            root: vault_root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let note_path = vault_root.join("2026-08-20.md");

        record_before(&vault, date, &note_path, "- [ ] secret\n").unwrap();

        let mode = fs::metadata(&undo_file).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "pre-existing undo file should be restricted to 0600, got {:04o}",
            mode & 0o777
        );

        let _ = fs::remove_file(&undo_file);
        let _ = fs::remove_dir_all(&vault_root);
        // SAFETY: tests are single threaded.
        unsafe {
            std::env::remove_var("OBSIDIAN_DAILY_QS_UNDO_PATH");
        }
    }
}
