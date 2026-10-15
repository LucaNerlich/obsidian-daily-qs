#![cfg(unix)]

//! `verify-bundle` runs in two modes. Feature PRs leave `omarchy/bin/` alone
//! and may carry a bundle whose source fingerprint trails `src/` (bundles are
//! refreshed only in the release PR, see AGENTS.md); CI runs those in
//! stale-tolerant mode and still attests the recorded hash and the ELF's
//! inspectability. Everything else — a change that edits `omarchy/bin/`, the
//! release tag, `make verify-bundle` — stays strict.
//!
//! These fixtures copy the real tree (scripts, src, the committed ELFs) and
//! only bend the fingerprint, so both modes are pinned end-to-end.

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
        "obsidian-verify-stale-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn copy_recursive(from: &Path, to: &Path) {
    if from.is_dir() {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            copy_recursive(&entry.path(), &to.join(entry.file_name()));
        }
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::copy(from, to).unwrap();
    }
}

/// A copy of the shipped tree with the bundle made stale: the fingerprint
/// covers every `src/*.rs` byte, so one appended comment is enough.
fn stale_fixture(name: &str) -> Dir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tmp_dir(name);
    for rel in [
        "omarchy",
        "scripts",
        "src",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "manifest.json",
    ] {
        copy_recursive(&root.join(rel), &dir.0.join(rel));
    }
    copy_recursive(
        &root.join(".github/workflows/ci.yml"),
        &dir.0.join(".github/workflows/ci.yml"),
    );

    let lib = dir.0.join("src/lib.rs");
    let mut text = fs::read_to_string(&lib).unwrap();
    text.push_str("\n// fixture: makes the committed bundle stale\n");
    fs::write(&lib, text).unwrap();

    dir
}

struct Outcome {
    ok: bool,
    text: String,
}

fn run_verify(dir: &Dir, allow_stale: bool, skip_rebuild: bool) -> Outcome {
    let mut cmd = Command::new("bash");
    cmd.arg(dir.0.join("scripts/verify-bundle.sh"))
        .arg("x86_64-unknown-linux-musl")
        .current_dir(&dir.0);
    if allow_stale {
        cmd.env("VERIFY_BUNDLE_ALLOW_STALE", "1");
    }
    if skip_rebuild {
        cmd.env("VERIFY_BUNDLE_SKIP_REBUILD", "1");
    }
    let out = cmd.output().expect("run verify-bundle.sh");
    Outcome {
        ok: out.status.success(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

#[test]
fn a_stale_bundle_passes_in_stale_tolerant_mode() {
    let dir = stale_fixture("tolerant");
    // Without VERIFY_BUNDLE_SKIP_REBUILD: staleness itself must skip the
    // rebuild, because a rebuild of the current source can only mismatch.
    let out = run_verify(&dir, true, false);
    assert!(out.ok, "stale-tolerant mode must pass: {}", out.text);
    assert!(
        out.text.contains("stale by design"),
        "the outcome must say why: {}",
        out.text
    );
    assert!(
        out.text.contains("rebuild skipped"),
        "the rebuild must be skipped: {}",
        out.text
    );
}

#[test]
fn the_inspect_only_ci_invocation_passes_and_reports_the_staleness() {
    let dir = stale_fixture("inspect-only");
    let out = run_verify(&dir, true, true);
    assert!(
        out.ok,
        "CI's stale-tolerant inspect must pass: {}",
        out.text
    );
    assert!(
        out.text.contains("stale by design"),
        "the outcome must say why: {}",
        out.text
    );
}

#[test]
fn strict_mode_still_refuses_a_stale_bundle() {
    let dir = stale_fixture("strict");
    let out = run_verify(&dir, false, true);
    assert!(
        !out.ok,
        "strict mode must refuse a stale bundle: {}",
        out.text
    );
    assert!(
        out.text
            .contains("stale relative to the tracked Rust source"),
        "unclear reason: {}",
        out.text
    );
}
