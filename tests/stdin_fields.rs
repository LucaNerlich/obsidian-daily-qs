#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

struct Vault(std::path::PathBuf);

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp_vault(name: &str) -> Vault {
    let dir = std::env::temp_dir().join(format!(
        "obsidian-stdin-fields-{}-{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Vault(dir)
}

fn run_raw(vault: &Vault, args: &[&str], stdin_json: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_obsidian-daily-qs"))
        .args(["--vault", vault.0.to_str().unwrap()])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(stdin_json.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn run_with_stdin(vault: &Vault, args: &[&str], stdin_json: &str) -> serde_json::Value {
    let output = run_raw(vault, args, stdin_json);
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn add_and_edit_read_sensitive_fields_from_stdin() {
    let vault = temp_vault("add-edit");
    let date = "2026-10-03";
    let secret = "private vault todo about salary";

    let added = run_with_stdin(
        &vault,
        &["add", "--date", date, "--stdin"],
        &format!(r#"{{"text":"{secret}"}}"#),
    );
    assert_eq!(added["state"], "ok");
    let todos = added["todos"].as_array().unwrap();
    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0]["text"], secret);
    let line = todos[0]["line"].as_u64().unwrap();

    let renamed = "private vault todo about bonus";
    let edited = run_with_stdin(
        &vault,
        &[
            "edit",
            "--date",
            date,
            "--line",
            &line.to_string(),
            "--stdin",
        ],
        &format!(r#"{{"text":"{renamed}","expectText":"{secret}"}}"#),
    );
    assert_eq!(edited["state"], "ok");
    let todos = edited["todos"].as_array().unwrap();
    assert_eq!(todos[0]["text"], renamed);

    // argv-style --text remains for CLI convenience
    let cli = Command::new(env!("CARGO_BIN_EXE_obsidian-daily-qs"))
        .args(["--vault", vault.0.to_str().unwrap()])
        .args(["add", "--date", date, "--text", "from-argv"])
        .output()
        .unwrap();
    assert!(cli.status.success());
    let snap: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(snap["state"], "ok");
    assert!(
        snap["todos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["text"] == "from-argv")
    );
}

#[test]
fn unknown_stdin_key_is_rejected_and_changes_nothing() {
    let vault = temp_vault("unknown-key");
    let date = "2026-10-03";

    let added = run_with_stdin(
        &vault,
        &["add", "--date", date, "--stdin"],
        r#"{"text":"keep me open"}"#,
    );
    let line = added["todos"][0]["line"].as_u64().unwrap();

    // `expect_text` is not a known field. Before `deny_unknown_fields` it was
    // dropped, which read as "no expectation given" and skipped the guard.
    let output = run_raw(
        &vault,
        &[
            "toggle",
            "--date",
            date,
            "--line",
            &line.to_string(),
            "--stdin",
        ],
        r#"{"expect_text":"keep me open"}"#,
    );
    let rejected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rejected["state"], "error");
    assert!(
        rejected["error"]
            .as_str()
            .unwrap()
            .contains("invalid stdin JSON"),
        "unexpected error: {}",
        rejected["error"]
    );

    // Nothing was toggled: the todo is still open.
    let status = run_with_stdin(&vault, &["status", "--date", date], "");
    assert_eq!(status["todos"][0]["checked"], false);
}

#[test]
fn toggle_expect_text_via_stdin() {
    let vault = temp_vault("toggle");
    let date = "2026-10-03";
    let secret = "expect-me-privately";

    let added = run_with_stdin(
        &vault,
        &["add", "--date", date, "--stdin"],
        &format!(r#"{{"text":"{secret}"}}"#),
    );
    let line = added["todos"][0]["line"].as_u64().unwrap();

    let toggled = run_with_stdin(
        &vault,
        &[
            "toggle",
            "--date",
            date,
            "--line",
            &line.to_string(),
            "--stdin",
        ],
        &format!(r#"{{"expectText":"{secret}"}}"#),
    );
    assert_eq!(toggled["state"], "ok");
    assert_eq!(toggled["todos"][0]["checked"], true);
}
