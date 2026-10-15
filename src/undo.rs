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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UndoFile {
    path: String,
    /// `None` means the file did not exist before the mutation, so undo
    /// deletes it again instead of restoring content.
    before: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UndoRecordMulti {
    vault: String,
    date: String,
    files: Vec<UndoFile>,
}

fn undo_path() -> Result<PathBuf, VaultError> {
    if let Ok(override_path) = std::env::var("OBSIDIAN_DAILY_QS_UNDO_PATH") {
        let trimmed = override_path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(
        cache_dir(std::env::var_os("XDG_CACHE_HOME"), std::env::var_os("HOME"))?
            .join("obsidian-daily-qs")
            .join("last-undo.json"),
    )
}

/// Pick the cache directory for the undo file.
///
/// Deliberately has **no** shared-directory fallback: the undo file holds the
/// previous contents of a note, so writing it into a world-writable location
/// (the old `/tmp` fallback) would expose vault text and let another local user
/// pre-plant a symlink for the write to follow. A missing HOME is an error, not
/// a reason to guess a location.
fn cache_dir(
    xdg_cache_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<PathBuf, VaultError> {
    // A set-but-empty or relative value must not be used: `PathBuf::from("")`
    // would make the undo file relative to the process CWD, which can be a
    // shared directory — the exposure this module exists to avoid. The XDG
    // spec requires an absolute path for `XDG_CACHE_HOME`.
    let usable = |value: std::ffi::OsString| {
        let path = PathBuf::from(value);
        (!path.as_os_str().is_empty() && path.is_absolute()).then_some(path)
    };
    xdg_cache_home
        .and_then(usable)
        .or_else(|| home.and_then(usable).map(|h| h.join(".cache")))
        .ok_or_else(|| {
            VaultError::Io("cannot locate the undo cache: set HOME or XDG_CACHE_HOME".into())
        })
}

/// Create a directory tree owner-only (0700) on Unix. The undo file itself is
/// already created 0600; this keeps the directory that holds it private too.
#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // `mode` applies only when the directory is created. A pre-existing cache
    // directory — from an older version, a root-run tool, or another local user
    // — keeps its loose mode otherwise, which would let someone replace the
    // undo file (the enabler for the symlink case in `write_undo_file`).
    // Tighten the leaf only, never recursively, so `~/.cache` itself is left
    // alone; a foreign-owned directory now fails with a clear `EPERM`.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

pub fn record_before(
    vault: &Vault,
    date: NaiveDate,
    path: &Path,
    before: &str,
) -> Result<(), VaultError> {
    record_before_multi(vault, date, &[(path, Some(before))])
}

/// Record the pre-mutation content of several notes (e.g. defer touches
/// tomorrow and today) so a single `undo` restores all of them. A `None`
/// `before` records that the file did not exist, so undo deletes it.
pub fn record_before_multi(
    vault: &Vault,
    date: NaiveDate,
    files: &[(&Path, Option<&str>)],
) -> Result<(), VaultError> {
    let dest = undo_path()?;
    // Harden only the directory this module manages. An explicit override path
    // (tests, or a user pointing `OBSIDIAN_DAILY_QS_UNDO_PATH` somewhere shared)
    // may legitimately live in a directory we do not own, and chmod'ing that
    // would be wrong — and would fail with EPERM on a shared temp dir.
    if let Some(parent) = dest.parent() {
        create_private_dir(parent).map_err(|e| {
            VaultError::Io(format!(
                "failed to secure undo dir {}: {e}",
                parent.display()
            ))
        })?;
    }
    record_before_multi_to(vault, date, files, &dest)
}

/// Same as [`record_before_multi`] but writes to an explicit path. Used by
/// tests so parallel test threads never share the process-global
/// `OBSIDIAN_DAILY_QS_UNDO_PATH` override.
pub fn record_before_multi_to(
    vault: &Vault,
    date: NaiveDate,
    files: &[(&Path, Option<&str>)],
    dest: &Path,
) -> Result<(), VaultError> {
    let record = UndoRecordMulti {
        vault: vault.root().display().to_string(),
        date: date.format("%Y-%m-%d").to_string(),
        files: files
            .iter()
            .map(|(path, before)| UndoFile {
                path: path.display().to_string(),
                before: before.map(|s| s.to_string()),
            })
            .collect(),
    };
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
    write_undo_file(dest, &json)
        .map_err(|e| VaultError::Io(format!("failed to write undo {}: {e}", dest.display())))?;

    // The undo file holds the previous note contents (the user's daily todos).
    // Restrict it to the owner so another local user cannot read it through a
    // world-traversable cache directory.
    if let Err(e) = restrict_undo_permissions(dest) {
        let _ = fs::remove_file(dest);
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

    // Never write through a symlink: `remove_file` unlinks one rather than
    // following it, so a pre-planted `last-undo.json -> victim` is removed
    // instead of being written through (which `create(true).truncate(true)` did
    // here, and the follow-up chmod followed it too). The remaining
    // check-then-open window needs write access to this file's directory, which
    // `create_private_dir` keeps owner-only — that is why the two go together.
    //
    // Unlike `create_new`, `create(true).truncate(true)` tolerates replacement
    // races on the single shared slot (several mutations, or test threads, can
    // write it concurrently) instead of failing one of them.
    if let Ok(meta) = fs::symlink_metadata(dest)
        && meta.file_type().is_symlink()
    {
        fs::remove_file(dest)?;
    }
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

/// True when `path` (or its nearest existing ancestor) canonicalizes to a
/// location inside the canonicalized vault root. Used before deleting a file
/// that an undone mutation created, so a tampered undo record cannot cause a
/// delete outside the vault.
fn path_inside_vault(vault_root: &Path, path: &Path) -> bool {
    let Ok(root) = vault_root.canonicalize() else {
        return false;
    };
    // The file itself may be the path to delete; fall back to its parent and
    // then to the nearest existing ancestor for canonicalization.
    if let Ok(real) = path.canonicalize() {
        return real.starts_with(&root);
    }
    let mut anchor = path.parent();
    while let Some(a) = anchor {
        if a.exists() {
            return a
                .canonicalize()
                .map(|real| real.starts_with(&root))
                .unwrap_or(false);
        }
        anchor = a.parent();
    }
    false
}

#[cfg(not(unix))]
fn restrict_undo_permissions(_dest: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Drop any pending undo record (used when a multi-note mutation fails after
/// recording `before` states but rolls everything back, leaving no net change).
pub fn discard() {
    if let Ok(dest) = undo_path() {
        discard_at(&dest);
    }
}

/// Same as [`discard`] but for an explicit undo path (tests).
pub fn discard_at(dest: &Path) {
    let _ = fs::remove_file(dest);
}

pub fn undo_last(vault: &Vault) -> Result<Snapshot, VaultError> {
    undo_last_from(vault, &undo_path()?)
}

/// Same as [`undo_last`] but reads a specific undo file. Used by tests with
/// per-test paths so parallel threads never share one global file.
pub fn undo_last_from(vault: &Vault, dest: &Path) -> Result<Snapshot, VaultError> {
    if !dest.exists() {
        return Err(VaultError::Io("nothing to undo".into()));
    }
    let raw = fs::read_to_string(dest)
        .map_err(|e| VaultError::Io(format!("failed to read undo {}: {e}", dest.display())))?;
    // Current format covers multi-note mutations like defer.
    if let Ok(record) = serde_json::from_str::<UndoRecordMulti>(&raw)
        && !record.files.is_empty()
    {
        let vault_s = vault.root().display().to_string();
        if record.vault != vault_s {
            return Err(VaultError::Io(
                "undo record is for a different vault; refusing to restore".into(),
            ));
        }
        let date = NaiveDate::parse_from_str(&record.date, "%Y-%m-%d")
            .map_err(|_| VaultError::Io(format!("invalid undo date {}", record.date)))?;
        for file in &record.files {
            let path = PathBuf::from(&file.path);
            match &file.before {
                Some(before) => crate::todos::write_atomic_public(vault.root(), &path, before)?,
                None => {
                    // File was created by the undone mutation; remove it again,
                    // but only when it is safely inside the vault.
                    if path.exists() && path_inside_vault(vault.root(), &path) {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
        let _ = fs::remove_file(dest);
        return read_snapshot(vault, date);
    }
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
    let _ = fs::remove_file(dest);
    read_snapshot(vault, date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn undo_cache_needs_home_or_xdg_cache_home() {
        // No shared-directory fallback: a missing HOME must be an error, not a
        // write into a world-writable location that holds note contents.
        let err = cache_dir(None, None).unwrap_err();
        assert!(err.to_string().contains("HOME or XDG_CACHE_HOME"));

        assert_eq!(
            cache_dir(Some(std::ffi::OsString::from("/xdg")), None).unwrap(),
            PathBuf::from("/xdg")
        );
        assert_eq!(
            cache_dir(None, Some(std::ffi::OsString::from("/home/u"))).unwrap(),
            PathBuf::from("/home/u/.cache")
        );
    }

    #[cfg(unix)]
    #[test]
    fn undo_cache_dir_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmp_path("undo-dir-mode");
        let nested = root.join("a").join("b");
        create_private_dir(&nested).unwrap();
        let mode = fs::metadata(&nested).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "undo cache dir must not be readable by others");
        let _ = fs::remove_dir_all(&root);
    }

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Unique temp path per call (process id + sequence + nanos), so parallel
    /// test threads never share an undo file or vault dir. Tests use the
    /// explicit-path undo variants and never touch the process-global
    /// `OBSIDIAN_DAILY_QS_UNDO_PATH` override.
    fn tmp_path(prefix: &str) -> PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "obsidian-daily-qs-{prefix}-{}-{n}-{nanos}",
            std::process::id()
        ))
    }

    fn tmp_undo_path() -> PathBuf {
        tmp_path("undo").with_extension("json")
    }

    #[cfg(unix)]
    #[test]
    fn undo_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let undo_file = tmp_undo_path();
        let _ = fs::remove_file(&undo_file);

        let vault_root = tmp_path("undo-vault");
        let _ = fs::remove_dir_all(&vault_root);
        fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault {
            root: vault_root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let note_path = vault_root.join("2026-08-20.md");

        record_before_multi_to(
            &vault,
            date,
            &[(&note_path, Some("- [ ] secret\n"))],
            &undo_file,
        )
        .unwrap();

        let mode = fs::metadata(&undo_file).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "undo file should be owner-only (0600), got {:04o}",
            mode & 0o777
        );

        let _ = fs::remove_file(&undo_file);
        let _ = fs::remove_dir_all(&vault_root);
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

        let vault_root = tmp_path("undo-vault-existing");
        let _ = fs::remove_dir_all(&vault_root);
        fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault {
            root: vault_root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let note_path = vault_root.join("2026-08-20.md");

        record_before_multi_to(
            &vault,
            date,
            &[(&note_path, Some("- [ ] secret\n"))],
            &undo_file,
        )
        .unwrap();

        let mode = fs::metadata(&undo_file).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "pre-existing undo file should be restricted to 0600, got {:04o}",
            mode & 0o777
        );

        let _ = fs::remove_file(&undo_file);
        let _ = fs::remove_dir_all(&vault_root);
    }

    fn defer_vault(root: PathBuf) -> (Vault, NaiveDate) {
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("Daily")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        (
            Vault {
                root,
                archive: None,
            },
            date,
        )
    }

    #[test]
    fn defer_undo_restores_both_notes() {
        let undo_file = tmp_undo_path();
        let _ = fs::remove_file(&undo_file);
        let root = tmp_path("defer-undo");
        let (vault, date) = defer_vault(root.clone());
        fs::write(
            root.join("Daily/2026-08-20.md"),
            "- [ ] keep\n- [ ] later\n",
        )
        .unwrap();
        fs::write(root.join("Daily/2026-08-21.md"), "# 2026-08-21\n").unwrap();

        crate::todos::defer_todo_to(
            &vault,
            date,
            2,
            Some("later"),
            false,
            None,
            Some(&undo_file),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("Daily/2026-08-20.md")).unwrap(),
            "- [ ] keep\n"
        );
        assert!(
            fs::read_to_string(root.join("Daily/2026-08-21.md"))
                .unwrap()
                .contains("- [ ] later")
        );

        undo_last_from(&vault, &undo_file).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("Daily/2026-08-20.md")).unwrap(),
            "- [ ] keep\n- [ ] later\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("Daily/2026-08-21.md")).unwrap(),
            "# 2026-08-21\n"
        );

        let _ = fs::remove_file(&undo_file);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn defer_undo_deletes_created_tomorrow_note() {
        let undo_file = tmp_undo_path();
        let _ = fs::remove_file(&undo_file);
        let root = tmp_path("defer-undo-created");
        let (vault, date) = defer_vault(root.clone());
        fs::write(
            root.join("Daily/2026-08-20.md"),
            "- [ ] keep\n- [ ] later\n",
        )
        .unwrap();
        assert!(!root.join("Daily/2026-08-21.md").exists());

        crate::todos::defer_todo_to(
            &vault,
            date,
            2,
            Some("later"),
            false,
            None,
            Some(&undo_file),
        )
        .unwrap();
        assert!(root.join("Daily/2026-08-21.md").exists());

        undo_last_from(&vault, &undo_file).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("Daily/2026-08-20.md")).unwrap(),
            "- [ ] keep\n- [ ] later\n"
        );
        assert!(
            !root.join("Daily/2026-08-21.md").exists(),
            "undo of a defer that created tomorrow's note should delete it again"
        );

        let _ = fs::remove_file(&undo_file);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn undo_restores_legacy_single_file_record() {
        let undo_file = tmp_undo_path();
        let vault_root = tmp_path("legacy-undo-vault");
        let _ = fs::remove_dir_all(&vault_root);
        fs::create_dir_all(&vault_root).unwrap();
        let note = vault_root.join("2026-08-20.md");
        fs::write(&note, "- [ ] changed\n").unwrap();
        let legacy = serde_json::json!({
            "vault": vault_root.display().to_string(),
            "date": "2026-08-20",
            "path": note.display().to_string(),
            "before": "- [ ] original\n",
        });
        fs::write(&undo_file, serde_json::to_string_pretty(&legacy).unwrap()).unwrap();
        let vault = Vault {
            root: vault_root.clone(),
            archive: None,
        };
        undo_last_from(&vault, &undo_file).unwrap();
        assert_eq!(fs::read_to_string(&note).unwrap(), "- [ ] original\n");
        assert!(!undo_file.exists());

        let _ = fs::remove_dir_all(&vault_root);
    }
}
