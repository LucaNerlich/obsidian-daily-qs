#![cfg(unix)]

//! The `verify-bundle` attestation gate must fail *closed*.
//!
//! `scripts/ci-needs-guard.sh` exit 0 means "the caller must abort": either the
//! job declares `needs:`, or the job could not be located in the file at all.
//! The original inline awk exited 1 unconditionally because `END { exit 1 }`
//! overrode the main rule's `exit 0`, so the gate was decorative and no test
//! noticed. These fixtures pin both halves.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tmp_dir(name: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("obsidian-ci-guard-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn guard_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-needs-guard.sh")
}

/// Run the guard against a workflow fixture. Returns (must_fail, combined output).
fn run_guard(name: &str, yaml: &str) -> (bool, String) {
    let dir = tmp_dir(name);
    let file = dir.0.join("ci.yml");
    fs::write(&file, yaml).unwrap();
    let out = Command::new("bash")
        .arg(guard_script())
        .arg(&file)
        .output()
        .expect("run the guard");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

const CLEAN: &str =
    "jobs:\n  verify-bundle:\n    runs-on: ubuntu-latest\n  linux:\n    needs: [changes]\n";

#[test]
fn clean_workflow_passes() {
    let (must_fail, text) = run_guard("clean", CLEAN);
    assert!(!must_fail, "no needs: on the job -> gate must pass: {text}");
}

#[test]
fn needs_on_the_job_aborts() {
    let (must_fail, text) = run_guard(
        "needs",
        "jobs:\n  verify-bundle:\n    needs: [linux]\n    runs-on: ubuntu-latest\n",
    );
    assert!(must_fail, "a needs: must abort the gate: {text}");
    assert!(
        text.contains("must not use needs"),
        "unclear reason: {text}"
    );
}

#[test]
fn needs_on_another_job_is_fine() {
    let (must_fail, _) = run_guard(
        "other-job",
        "jobs:\n  linux:\n    needs: [changes]\n  verify-bundle:\n    runs-on: ubuntu-latest\n",
    );
    assert!(!must_fail, "only the verify-bundle job is guarded");
}

#[test]
fn a_job_that_cannot_be_located_aborts() {
    // The whole point of the fail-closed rewrite: a rename or move must not
    // silently disarm the gate.
    let (must_fail, text) = run_guard(
        "renamed",
        "jobs:\n  attest-bundle:\n    runs-on: ubuntu-latest\n",
    );
    assert!(must_fail, "an unlocatable job must abort: {text}");
    assert!(text.contains("not defined in"), "unclear reason: {text}");
}

#[test]
fn crlf_does_not_hide_a_needs() {
    let (must_fail, text) = run_guard(
        "crlf",
        "jobs:\r\n  verify-bundle:\r\n    needs: [linux]\r\n    runs-on: ubuntu-latest\r\n",
    );
    assert!(must_fail, "CRLF must not hide the job or its keys: {text}");
}

#[test]
fn quoted_and_commented_job_keys_are_found() {
    let (quoted, qtext) = run_guard(
        "quoted",
        "jobs:\n  \"verify-bundle\":\n    needs: [linux]\n",
    );
    assert!(quoted, "a quoted job key must still be guarded: {qtext}");

    let (commented, ctext) = run_guard(
        "commented",
        "jobs:\n  verify-bundle: # attestation\n    needs: [linux]\n",
    );
    assert!(
        commented,
        "a trailing comment must still be guarded: {ctext}"
    );
}

#[test]
fn the_real_workflow_is_clean() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/ci.yml");
    let out = Command::new("bash")
        .arg(guard_script())
        .arg(&path)
        .output()
        .expect("run the guard");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "the shipped ci.yml must pass the guard: {text}"
    );
}
