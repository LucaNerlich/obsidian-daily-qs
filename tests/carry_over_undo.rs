#![cfg(unix)]

//! Carry-over moves todos between two notes. Because the undo record is a
//! single slot, it has to name *both* notes: recording only the target let one
//! undo remove the carried copy while the source stayed emptied, so the todos
//! lived in neither note.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tmp_dir(name: &str) -> Dir {
    let dir =
        std::env::temp_dir().join(format!("obsidian-carry-over-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_obsidian-daily-qs")
}

/// A failed source removal leaves the vault half-moved: the target holds the
/// carried copies and the source still does too. The undo record written before
/// the removal must survive so one undo repairs it.
#[test]
fn a_failed_removal_keeps_the_undo_record_and_can_be_repaired() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tmp_dir("failed-removal");
    let vault = dir.0.join("vault");
    fs::create_dir_all(vault.join(".obsidian")).unwrap();
    fs::write(
        vault.join(".obsidian/daily-notes.json"),
        r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
    )
    .unwrap();
    let today = vault.join("Daily/2026-10-15.md");
    fs::create_dir_all(today.parent().unwrap()).unwrap();
    fs::write(&today, "# 2026-10-15\n").unwrap();

    // The source is an archived note, so it can sit in a directory that is not
    // writable while the target stays writable.
    let archive = vault.join("_archive");
    fs::create_dir_all(&archive).unwrap();
    let yesterday = archive.join("2026-10-14.md");
    fs::write(
        &yesterday,
        "# 2026-10-14\n\n- [ ] leftover from yesterday\n",
    )
    .unwrap();

    let undo_file = dir.0.join("undo.json");
    let run = |args: &[&str]| {
        Command::new(bin())
            .args(["--vault", vault.to_str().unwrap()])
            .args(args)
            .env("OBSIDIAN_DAILY_QS_UNDO_PATH", &undo_file)
            .output()
            .unwrap()
    };

    fs::set_permissions(&archive, fs::Permissions::from_mode(0o555)).unwrap();
    let carried = run(&[
        "carry-over",
        "--date",
        "2026-10-15",
        "--archive-folder",
        "_archive",
    ]);
    let snap: serde_json::Value = serde_json::from_slice(&carried.stdout).unwrap();
    assert_eq!(
        snap["state"], "error",
        "the removal should have failed: {snap}"
    );
    assert!(
        undo_file.is_file(),
        "the record must survive a failed removal, otherwise the half-move cannot be undone"
    );
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o755)).unwrap();

    // The half-move is visible — the target has the copy — and one undo puts the
    // vault back to its pre-carry state.
    assert!(fs::read_to_string(&today).unwrap().contains("leftover"));
    let undone = run(&["undo"]);
    assert!(
        undone.status.success(),
        "undo failed: {}",
        String::from_utf8_lossy(&undone.stderr)
    );
    assert!(
        fs::read_to_string(&yesterday)
            .unwrap()
            .contains("leftover from yesterday"),
        "undo must leave the source note as it was"
    );
    assert!(
        !fs::read_to_string(&today).unwrap().contains("leftover"),
        "undo must remove the carried copy from the target"
    );
}

#[test]
fn carry_over_then_undo_restores_both_notes() {
    let dir = tmp_dir("undo");
    let vault = dir.0.join("vault");
    fs::create_dir_all(&vault).unwrap();
    // Per-test undo file: never the process-global slot.
    let undo_file = dir.0.join("undo.json");

    let yesterday = vault.join("2026-10-14.md");
    let today = vault.join("2026-10-15.md");
    fs::write(
        &yesterday,
        "# 2026-10-14\n\n- [ ] leftover from yesterday\n",
    )
    .unwrap();
    fs::write(&today, "# 2026-10-15\n").unwrap();

    let run = |args: &[&str]| {
        Command::new(bin())
            .args(["--vault", vault.to_str().unwrap()])
            .args(args)
            .env("OBSIDIAN_DAILY_QS_UNDO_PATH", &undo_file)
            .output()
            .unwrap()
    };

    let carried = run(&["carry-over", "--date", "2026-10-15"]);
    assert!(
        carried.status.success(),
        "carry-over failed: {}",
        String::from_utf8_lossy(&carried.stderr)
    );
    let snap: serde_json::Value = serde_json::from_slice(&carried.stdout).unwrap();
    assert_eq!(snap["state"], "ok");
    assert_eq!(snap["openCount"], 1, "the todo should have been carried");
    assert!(
        fs::read_to_string(&today)
            .unwrap()
            .contains("leftover from yesterday"),
        "carried todo missing from the target note"
    );
    assert!(
        !fs::read_to_string(&yesterday)
            .unwrap()
            .contains("leftover from yesterday"),
        "carry-over must remove the todo from the source note"
    );

    // One undo restores both notes.
    let undone = run(&["undo"]);
    assert!(
        undone.status.success(),
        "undo failed: {}",
        String::from_utf8_lossy(&undone.stderr)
    );
    assert!(
        fs::read_to_string(&yesterday)
            .unwrap()
            .contains("leftover from yesterday"),
        "undo must put the todo back in the source note"
    );
    assert!(
        !fs::read_to_string(&today)
            .unwrap()
            .contains("leftover from yesterday"),
        "undo must remove the carried copy from the target note"
    );
}
