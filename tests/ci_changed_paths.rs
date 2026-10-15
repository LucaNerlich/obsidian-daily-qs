#![cfg(unix)]

//! `scripts/ci-changed-paths.sh` decides which CI jobs run (`rust`) and
//! whether the committed bundle must be fresh (`bundle`). The fail-closed half
//! matters: whenever the diff base cannot be resolved the answer must be
//! `true` for both, because a silent `false` would skip the Rust tests or
//! disarm the bundle attestation.

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
    let dir = std::env::temp_dir().join(format!(
        "obsidian-changed-paths-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci-changed-paths.sh")
}

fn git(dir: &Dir, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(&dir.0)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Dir, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
    git(dir, &["rev-parse", "HEAD"])
}

/// A tiny repo: docs-only commit, then a Rust commit, then a bundle commit.
fn repo(name: &str) -> (Dir, String, String, String) {
    let dir = tmp_dir(name);
    git(&dir, &["init", "-q"]);

    fs::create_dir_all(dir.0.join("docs")).unwrap();
    fs::write(dir.0.join("docs/NOTES.md"), "notes\n").unwrap();
    let docs = commit(&dir, "docs");

    fs::create_dir_all(dir.0.join("src")).unwrap();
    fs::write(dir.0.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    let rust = commit(&dir, "rust");

    fs::create_dir_all(dir.0.join("omarchy/bin")).unwrap();
    fs::write(dir.0.join("omarchy/bin/obsidian-daily-qs-x86_64"), "elf\n").unwrap();
    let bundle = commit(&dir, "bundle");

    (dir, docs, rust, bundle)
}

fn run(dir: &Dir, event: &str, pr_base: &str, push_before: &str, head: &str) -> (String, String) {
    let out = Command::new("bash")
        .arg(script())
        .args([event, pr_base, push_before, head])
        .current_dir(&dir.0)
        .output()
        .expect("run ci-changed-paths.sh");
    assert!(
        out.status.success(),
        "ci-changed-paths.sh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let flag = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_else(|| panic!("missing {key} in output: {text}"))
            .to_string()
    };
    (flag("rust="), flag("bundle="))
}

#[test]
fn a_rust_change_needs_the_rust_jobs_but_not_a_fresh_bundle() {
    let (dir, docs, rust, _) = repo("rust-change");
    let (r, b) = run(&dir, "pull_request", &docs, "", &rust);
    assert_eq!(r, "true");
    assert_eq!(b, "false");
}

#[test]
fn a_bundle_change_needs_strict_verification() {
    let (dir, _, rust, bundle) = repo("bundle-change");
    // The bundle also matches the `rust` pattern: verify-bundle is not gated
    // on it, but the Rust jobs should run when only omarchy/bin/ changed too.
    let (r, b) = run(&dir, "pull_request", &rust, "", &bundle);
    assert_eq!(r, "true");
    assert_eq!(b, "true");
}

#[test]
fn push_events_use_the_before_sha_as_base() {
    let (dir, _, rust, bundle) = repo("push-event");
    let (r, b) = run(&dir, "push", "", &rust, &bundle);
    assert_eq!(r, "true");
    assert_eq!(b, "true");
}

#[test]
fn a_docs_only_change_skips_both() {
    let (dir, docs, _, _) = repo("docs-only");
    let (r, b) = run(&dir, "pull_request", &docs, "", &docs);
    assert_eq!(r, "false");
    assert_eq!(b, "false");
}

#[test]
fn an_unresolvable_base_fails_closed() {
    let (dir, _, _, bundle) = repo("unresolvable");
    let (r, b) = run(
        &dir,
        "push",
        "",
        "0000000000000000000000000000000000000000",
        &bundle,
    );
    assert_eq!(
        r, "true",
        "an unresolvable base must not skip the Rust jobs"
    );
    assert_eq!(b, "true", "an unresolvable base must demand a fresh bundle");

    let (r, b) = run(
        &dir,
        "pull_request",
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        "",
        &bundle,
    );
    assert_eq!(r, "true");
    assert_eq!(b, "true");
}

#[test]
fn workflow_dispatch_fails_closed() {
    let (dir, _, _, bundle) = repo("dispatch");
    let (r, b) = run(&dir, "workflow_dispatch", "", "", &bundle);
    assert_eq!(r, "true");
    assert_eq!(b, "true");
}
