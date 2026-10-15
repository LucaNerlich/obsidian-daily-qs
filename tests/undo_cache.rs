#![cfg(unix)]

//! Where the undo file is written is a privacy property: it holds the previous
//! contents of a note. These tests drive the real binary — the cache logic is
//! only reachable through it — with a controlled environment, so a regression
//! that writes into a shared or attacker-reachable location fails a test rather
//! than only a review.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tmp_dir(name: &str) -> Dir {
    let dir =
        std::env::temp_dir().join(format!("obsidian-undo-cache-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

/// Run `add` with the given environment, from `dir` as the working directory.
/// `env_clear` removes HOME/XDG_CACHE_HOME entirely.
fn add_in(dir: &Path, set: &[(&str, &str)], clear: bool) -> serde_json::Value {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_obsidian-daily-qs"));
    cmd.args([
        "--vault",
        vault.to_str().unwrap(),
        "add",
        "--date",
        "2026-10-04",
        "--stdin",
    ])
    .current_dir(dir)
    .env_remove("OBSIDIAN_DAILY_QS_UNDO_PATH")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    if clear {
        cmd.env_clear();
    }
    for (key, value) in set {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"text":"private todo"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("no JSON on stdout: {e} / {:?}", out))
}

fn cache_file(home: &Path) -> PathBuf {
    home.join(".cache")
        .join("obsidian-daily-qs")
        .join("last-undo.json")
}

#[test]
fn no_home_writes_nothing_to_the_working_directory() {
    let dir = tmp_dir("no-home");
    let snap = add_in(&dir.0, &[], true);
    assert_eq!(snap["state"], "error");
    assert!(
        snap["error"]
            .as_str()
            .unwrap()
            .contains("HOME or XDG_CACHE_HOME"),
        "unexpected error: {}",
        snap["error"]
    );
    assert!(
        !dir.0.join("obsidian-daily-qs").exists(),
        "the undo file must never be written relative to the CWD"
    );
    assert!(!dir.0.join(".cache").exists());
}

#[test]
fn empty_xdg_cache_home_falls_back_to_home_instead_of_the_cwd() {
    let dir = tmp_dir("empty-xdg");
    let home = dir.0.join("home");
    fs::create_dir_all(&home).unwrap();
    let snap = add_in(
        &dir.0,
        &[("XDG_CACHE_HOME", ""), ("HOME", home.to_str().unwrap())],
        true,
    );
    assert_eq!(snap["state"], "ok", "{snap}");
    assert!(
        cache_file(&home).is_file(),
        "the undo file should have landed under $HOME/.cache"
    );
    assert!(
        !dir.0.join("obsidian-daily-qs").exists(),
        "an empty XDG_CACHE_HOME must not mean the CWD"
    );
}

#[test]
fn a_planted_symlink_is_not_written_through() {
    let dir = tmp_dir("symlink");
    let home = dir.0.join("home");
    let cache = home.join(".cache").join("obsidian-daily-qs");
    fs::create_dir_all(&cache).unwrap();
    let victim = dir.0.join("victim.txt");
    fs::write(&victim, "important\n").unwrap();
    std::os::unix::fs::symlink(&victim, cache.join("last-undo.json")).unwrap();

    let snap = add_in(&dir.0, &[("HOME", home.to_str().unwrap())], true);
    assert_eq!(snap["state"], "ok", "{snap}");

    assert_eq!(
        fs::read_to_string(&victim).unwrap(),
        "important\n",
        "the write followed a planted symlink out of the cache directory"
    );
    let meta = fs::symlink_metadata(cache.join("last-undo.json")).unwrap();
    assert!(
        meta.file_type().is_file(),
        "the slot should be a regular file after the write"
    );
}

#[test]
fn a_loose_cache_directory_is_tightened() {
    let dir = tmp_dir("loose-dir");
    let home = dir.0.join("home");
    let cache = home.join(".cache").join("obsidian-daily-qs");
    fs::create_dir_all(&cache).unwrap();
    fs::set_permissions(&cache, fs::Permissions::from_mode(0o777)).unwrap();

    let snap = add_in(&dir.0, &[("HOME", home.to_str().unwrap())], true);
    assert_eq!(snap["state"], "ok", "{snap}");

    let mode = fs::metadata(&cache).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o700,
        "a pre-existing cache directory must be tightened, not trusted"
    );
    let file_mode = fs::metadata(cache_file(&home))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(file_mode, 0o600);
}
