//! Poll today's daily note and emit JSON snapshots when it changes.

use std::fs;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use chrono::Local;

use crate::config::Vault;
use crate::status::Snapshot;
use crate::todos::read_snapshot;

const POLL: Duration = Duration::from_secs(1);

/// Stream snapshots: one immediately, then whenever path/mtime/content changes.
pub fn watch() {
    let mut last_key: Option<String> = None;
    loop {
        let snap = current_snapshot();
        let key = snapshot_key(&snap);
        if last_key.as_ref() != Some(&key) {
            if !emit(&snap) {
                // Consumer is gone (shell crashed or exited without reaping
                // us); keep polling would leak this process.
                return;
            }
            last_key = Some(key);
        }
        thread::sleep(POLL);
    }
}

pub fn current_snapshot() -> Snapshot {
    match Vault::from_env() {
        Ok(vault) => {
            let date = Local::now().date_naive();
            match read_snapshot(&vault, date) {
                Ok(snap) => snap,
                Err(err) => Snapshot::error(err.to_string()),
            }
        }
        Err(err) => Snapshot::error(err.to_string()),
    }
}

pub fn snapshot_for_date(date: chrono::NaiveDate) -> Snapshot {
    match Vault::from_env() {
        Ok(vault) => match read_snapshot(&vault, date) {
            Ok(snap) => snap,
            Err(err) => Snapshot::error(err.to_string()),
        },
        Err(err) => Snapshot::error(err.to_string()),
    }
}

fn snapshot_key(snap: &Snapshot) -> String {
    // The serialized body covers what mtime/len miss: same-size checkbox
    // toggles on coarse-mtime filesystems and carryOverCount changes caused
    // by edits to yesterday's note.
    let body = serde_json::to_string(snap).unwrap_or_default();
    if let (Some(path), Some(true)) = (snap.path.as_ref(), snap.exists) {
        let mtime = fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        format!("ok:{path}:{mtime}:{len}:{body}")
    } else {
        body
    }
}

/// Returns false when stdout is broken (EPIPE), so callers can exit.
fn emit(snap: &Snapshot) -> bool {
    let line = serde_json::to_string(snap).expect("snapshot serializes");
    let mut out = io::stdout().lock();
    let mut broken = writeln!(out, "{line}").is_err();
    broken |= out.flush().is_err();
    !broken
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::State;

    fn snap_with_carry(carry: usize) -> Snapshot {
        Snapshot {
            state: State::Ok,
            date: Some("2026-08-21".into()),
            path: Some("/nonexistent/vault/Daily/2026-08-21.md".into()),
            exists: Some(true),
            open_count: Some(0),
            done_count: Some(0),
            todos: Some(Vec::new()),
            obsidian_uri: None,
            carry_over_count: Some(carry),
            is_today: Some(true),
            error: None,
        }
    }

    #[test]
    fn key_changes_when_only_carry_over_count_changes() {
        // The path does not exist, so mtime/len are constant here; the key
        // must still differ when yesterday's open count changed.
        assert_ne!(
            snapshot_key(&snap_with_carry(0)),
            snapshot_key(&snap_with_carry(3))
        );
        assert_eq!(
            snapshot_key(&snap_with_carry(2)),
            snapshot_key(&snap_with_carry(2))
        );
    }
}
