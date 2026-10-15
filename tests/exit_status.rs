#![cfg(unix)]

//! One-shot commands must report failure through the process status, not only
//! in the JSON body. `cmd || fallback`, CI steps and shell wrappers otherwise
//! treat a failed mutation as success.

use std::fs;
use std::process::{Command, Stdio};

struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tmp_dir(name: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!(
        "obsidian-exit-status-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_obsidian-daily-qs")
}

#[test]
fn success_snapshot_exits_zero() {
    let dir = tmp_dir("ok");
    let out = Command::new(bin())
        .args(["--vault", dir.0.to_str().unwrap(), "status"])
        .output()
        .unwrap();
    let snap: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(snap["state"], "ok");
    assert!(out.status.success(), "status={:?}", out.status);
}

#[test]
fn error_snapshot_exits_non_zero() {
    let dir = tmp_dir("err");
    // Without HOME/XDG_CACHE_HOME the undo cache cannot be located, so the
    // command returns an error snapshot.
    let out = Command::new(bin())
        .args(["--vault", dir.0.to_str().unwrap(), "undo"])
        .env_remove("HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("OBSIDIAN_DAILY_QS_UNDO_PATH")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let snap: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(snap["state"], "error");
    assert!(
        !out.status.success(),
        "an error snapshot must not exit 0 (status={:?})",
        out.status
    );
}

#[test]
fn a_closed_stdout_does_not_panic() {
    let dir = tmp_dir("epipe");
    let mut child = Command::new(bin())
        .args(["--vault", dir.0.to_str().unwrap(), "status"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Close the read end before the child writes: the write fails with EPIPE.
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.code().is_some(),
        "must exit normally rather than die from a signal: {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "closed stdout must not panic: {stderr}"
    );
}
