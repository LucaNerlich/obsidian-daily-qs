#![cfg(unix)]

//! One-shot commands must report failure through the process status, not only
//! in the JSON body. `cmd || fallback`, CI steps and shell wrappers otherwise
//! treat a failed mutation as success — but a reader that closes the pipe early
//! consumed what it asked for and must not be reported as a failure.

use std::fs;
use std::io::Write;
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
    // A fresh vault has no undo record, so the command returns an error
    // snapshot. `OBSIDIAN_DAILY_QS_UNDO_PATH` is cleared so the developer's own
    // undo file is never read.
    let out = Command::new(bin())
        .args(["--vault", dir.0.to_str().unwrap(), "undo"])
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
fn a_closed_stdout_reports_success_and_does_not_panic() {
    let dir = tmp_dir("epipe");
    let mut child = Command::new(bin())
        .args([
            "--vault",
            dir.0.to_str().unwrap(),
            "add",
            "--date",
            "2026-10-04",
            "--stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Close the read end before the child can write, then release it, so the
    // write deterministically fails with EPIPE rather than racing.
    let mut stdin = child.stdin.take().unwrap();
    drop(child.stdout.take());
    stdin.write_all(br#"{"text":"x"}"#).unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "closed stdout must not panic: {stderr}"
    );
    assert!(
        out.status.success(),
        "a reader that closed early is not a failure: {:?}",
        out.status
    );
}

/// A write that genuinely fails to reach the caller must be reported. `/dev/full`
/// is Linux-only, and CI runs the tests on Linux.
#[cfg(target_os = "linux")]
#[test]
fn a_failed_write_exits_non_zero() {
    let dir = tmp_dir("enospc");
    let sink = fs::File::create("/dev/full").unwrap();
    let status = Command::new(bin())
        .args(["--vault", dir.0.to_str().unwrap(), "status"])
        .stdout(sink)
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "output that did not reach the caller must not exit 0"
    );
}
