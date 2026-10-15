#![cfg(unix)]

//! The vault root (and archive pattern) can travel on stdin (#102).
//!
//! The widget launches every backend process with `--stdin` and hands it
//! `{"vault": …}` instead of `--vault`, because `/proc/<pid>/cmdline` is
//! world-readable on systems without hidepid — and a watch process would keep
//! the absolute path there for the whole session. These tests exercise the
//! channel from the outside: no `--vault` on argv, no `OBSIDIAN_VAULT_ROOT` in
//! the environment, vault only in the payload.

use std::fs;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

struct Vault(std::path::PathBuf);

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp_vault(name: &str) -> Vault {
    let dir = std::env::temp_dir().join(format!(
        "obsidian-vault-stdin-{}-{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".obsidian")).unwrap();
    fs::write(
        dir.join(".obsidian/daily-notes.json"),
        r#"{"folder":"dailies","format":"YYYY-MM-DD"}"#,
    )
    .unwrap();
    Vault(dir)
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_obsidian-daily-qs")
}

/// Run the binary with no vault on argv and no vault in the environment: the
/// payload is the only source. Returns (status, parsed stdout, stderr).
fn run_with_payload(
    args: &[&str],
    payload: &str,
) -> (std::process::ExitStatus, serde_json::Value, String) {
    let mut child = Command::new(bin())
        .args(args)
        .env_remove("OBSIDIAN_VAULT_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(payload.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let snap = serde_json::from_str(stdout.trim()).unwrap_or(serde_json::Value::Null);
    (
        out.status,
        snap,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write_note(vault: &Vault, relative: &str, body: &str) {
    let path = vault.0.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn status_resolves_the_vault_from_stdin() {
    let vault = temp_vault("status");
    write_note(
        &vault,
        "dailies/2026-10-15.md",
        "# 2026-10-15\n\n## Todos\n- [ ] ship it\n",
    );

    let payload = format!(r#"{{"vault":{}}}"#, serde_json::json!(vault.0));
    let (status, snap, stderr) =
        run_with_payload(&["status", "--date", "2026-10-15", "--stdin"], &payload);

    assert!(status.success(), "stderr={stderr} stdout={snap}");
    assert_eq!(snap["state"], "ok");
    assert_eq!(snap["openCount"], 1);
    assert!(
        snap["path"]
            .as_str()
            .unwrap()
            .starts_with(vault.0.to_str().unwrap()),
        "resolved the wrong vault: {}",
        snap["path"]
    );
}

#[test]
fn week_resolves_the_vault_from_stdin() {
    let vault = temp_vault("week");
    write_note(
        &vault,
        "dailies/2026-10-15.md",
        "# 2026-10-15\n\n## Todos\n- [ ] ship it\n- [x] done\n",
    );

    let payload = format!(r#"{{"vault":{}}}"#, serde_json::json!(vault.0));
    let (status, snap, stderr) =
        run_with_payload(&["week", "--date", "2026-10-15", "--stdin"], &payload);

    assert!(status.success(), "stderr={stderr}");
    assert_eq!(snap["state"], "ok");
    let thursday = snap["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["date"] == "2026-10-15")
        .expect("day in week");
    assert_eq!(thursday["openCount"], 1);
    assert_eq!(thursday["doneCount"], 1);
}

#[test]
fn the_archive_pattern_travels_on_stdin_too() {
    let vault = temp_vault("archive");
    write_note(
        &vault,
        "dailies/_archive/2026/2026-10-15.md",
        "# 2026-10-15\n\n## Todos\n- [ ] archived but open\n",
    );

    // Without the pattern the live folder is all the backend knows about.
    let payload = format!(r#"{{"vault":{}}}"#, serde_json::json!(vault.0));
    let (_, snap, _) = run_with_payload(&["status", "--date", "2026-10-15", "--stdin"], &payload);
    assert_eq!(snap["exists"], false, "primary note is missing");

    let payload = format!(
        r#"{{"vault":{},"archiveFolder":"dailies/_archive/YYYY"}}"#,
        serde_json::json!(vault.0)
    );
    let (status, snap, stderr) =
        run_with_payload(&["status", "--date", "2026-10-15", "--stdin"], &payload);
    assert!(status.success(), "stderr={stderr}");
    assert_eq!(
        snap["exists"], true,
        "archive folder from stdin was ignored"
    );
    assert_eq!(snap["openCount"], 1);
    assert!(snap["path"].as_str().unwrap().contains("_archive/2026"));
}

#[test]
fn a_mutation_takes_vault_and_text_from_one_payload() {
    let vault = temp_vault("add");
    let payload = format!(
        r#"{{"vault":{},"text":"from the stdin channel"}}"#,
        serde_json::json!(vault.0)
    );

    let (status, snap, stderr) =
        run_with_payload(&["add", "--date", "2026-10-15", "--stdin"], &payload);
    assert!(status.success(), "stderr={stderr}");
    assert_eq!(snap["state"], "ok");
    assert_eq!(snap["openCount"], 1);

    let note = fs::read_to_string(vault.0.join("dailies/2026-10-15.md")).unwrap();
    assert!(note.contains("from the stdin channel"), "note={note}");
}

#[test]
fn watch_streams_with_the_vault_on_stdin() {
    let vault = temp_vault("watch");
    write_note(
        &vault,
        "dailies/2026-10-15.md",
        "# 2026-10-15\n\n## Todos\n- [ ] watched\n",
    );

    let mut child = Command::new(bin())
        .args(["watch", "--stdin"])
        .env_remove("OBSIDIAN_VAULT_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(format!(r#"{{"vault":{}}}"#, serde_json::json!(vault.0)).as_bytes())
            .unwrap();
    } // dropped: the watch now sees EOF on stdin, like the widget's closed pipe

    // First snapshot must arrive without waiting for a poll tick; the timeout
    // keeps a hang from failing the suite only after the harness throws.
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let first = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("watch emitted nothing with the vault on stdin");
    let _ = child.kill();
    let _ = child.wait();
    reader.join().unwrap();

    let snap: serde_json::Value = serde_json::from_str(first.trim()).expect("snapshot json");
    assert_eq!(snap["state"], "ok");
    assert_eq!(snap["openCount"], 1);
    assert!(
        snap["path"]
            .as_str()
            .unwrap()
            .starts_with(vault.0.to_str().unwrap())
    );
}

#[test]
fn an_explicit_vault_argument_wins_over_the_payload() {
    let vault = temp_vault("precedence");
    write_note(&vault, "dailies/2026-10-15.md", "# 2026-10-15\n");

    let out = Command::new(bin())
        .args([
            "--vault",
            vault.0.to_str().unwrap(),
            "status",
            "--date",
            "2026-10-15",
            "--stdin",
        ])
        .env_remove("OBSIDIAN_VAULT_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map(|mut child| {
            let mut stdin = child.stdin.take().expect("stdin");
            stdin
                .write_all(br#"{"vault":"/definitely/not/a/vault"}"#)
                .unwrap();
            // Close the pipe before waiting: the backend reads stdin to EOF.
            drop(stdin);
            child.wait_with_output().unwrap()
        })
        .unwrap();

    let snap: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(snap["state"], "ok", "argv --vault must win: {snap}");
}

#[test]
fn a_field_the_command_does_not_take_is_rejected() {
    let vault = temp_vault("unexpected");
    let payload = format!(
        r#"{{"vault":{},"text":"not a status field"}}"#,
        serde_json::json!(vault.0)
    );

    let (status, snap, _) = run_with_payload(&["status", "--stdin"], &payload);
    assert!(!status.success(), "an ignored field must not succeed");
    assert_eq!(snap["state"], "error");
    let error = snap["error"].as_str().unwrap();
    assert!(error.contains("text"), "unclear error: {error}");
}

#[test]
fn an_unknown_key_is_still_rejected() {
    let vault = temp_vault("unknown-key");
    // A typo ("vaults") must not silently fall back to env/primary resolution.
    let payload = format!(r#"{{"vaults":{}}}"#, serde_json::json!(vault.0));

    let (status, snap, _) =
        run_with_payload(&["status", "--date", "2026-10-15", "--stdin"], &payload);
    assert!(!status.success());
    assert_eq!(snap["state"], "error");
    assert!(
        snap["error"]
            .as_str()
            .unwrap()
            .contains("invalid stdin JSON"),
        "unknown key should be refused: {snap}"
    );
}

#[test]
fn a_missing_vault_in_the_payload_falls_back_to_a_clear_error() {
    // The widget sends no `vault` while the setup path is empty; the backend
    // must say so instead of reading some other vault.
    let (status, snap, _) = run_with_payload(&["status", "--stdin"], "{}");
    assert!(!status.success());
    assert_eq!(snap["state"], "error");
    assert_eq!(snap["errorCode"], "missing_vault");
}

#[test]
fn text_and_stdin_still_conflict() {
    let out = Command::new(bin())
        .args(["add", "--text", "x", "--stdin"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success(), "conflicting inputs must be refused");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--stdin") || stderr.contains("cannot be used with"),
        "clap should explain the conflict: {stderr}"
    );
}
