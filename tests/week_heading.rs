#![cfg(unix)]

//! `week --heading` must count the same todos the day view shows (#62).
//!
//! The week strip and the panel read the same note, so with a configured
//! `todoHeading` an unfiltered strip reports checkboxes the day view hides and
//! the two disagree on screen.

use std::fs;
use std::process::{Command, Stdio};

struct Vault(std::path::PathBuf);

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp_vault(name: &str) -> Vault {
    let dir = std::env::temp_dir().join(format!(
        "obsidian-week-heading-{}-{}",
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

/// 2026-10-14 is a Wednesday; the week runs Mon 12th – Sun 18th.
const WEDNESDAY: &str = "2026-10-14";

fn write_note(vault: &Vault) {
    let note = vault.0.join("dailies").join(format!("{WEDNESDAY}.md"));
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    fs::write(
        &note,
        format!(
            "# {WEDNESDAY}\n\n\
             ## Todos\n\
             - [ ] ship the plugin\n\
             - [x] write the notes\n\n\
             ## Log\n\
             - [ ] something from the journal\n\
             - [ ] another journal line\n"
        ),
    )
    .unwrap();
}

fn run(vault: &Vault, args: &[&str]) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_obsidian-daily-qs"))
        .args(["--vault", vault.0.to_str().unwrap()])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn day<'a>(week: &'a serde_json::Value, date: &str) -> &'a serde_json::Value {
    week["days"]
        .as_array()
        .expect("week days")
        .iter()
        .find(|d| d["date"] == date)
        .expect("day in week")
}

#[test]
fn week_counts_only_the_configured_heading() {
    let vault = temp_vault("filtered");
    write_note(&vault);

    let week = run(&vault, &["week", "--date", WEDNESDAY, "--heading", "Todos"]);
    let wed = day(&week, WEDNESDAY);
    assert_eq!(wed["openCount"], 1, "only the Todos checkbox is open");
    assert_eq!(wed["doneCount"], 1);

    // The journal section stays out of the strip's other days too.
    let open_total: u64 = week["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["openCount"].as_u64().unwrap())
        .sum();
    assert_eq!(open_total, 1, "no other day may pick up the journal lines");
}

#[test]
fn the_week_matches_the_day_view_it_sits_next_to() {
    let vault = temp_vault("parity");
    write_note(&vault);

    let day_view = run(
        &vault,
        &["status", "--date", WEDNESDAY, "--heading", "Todos"],
    );
    let week = run(&vault, &["week", "--date", WEDNESDAY, "--heading", "Todos"]);
    let wed = day(&week, WEDNESDAY);
    assert_eq!(wed["openCount"], day_view["openCount"]);
    assert_eq!(wed["doneCount"], day_view["doneCount"]);
}

#[test]
fn without_a_heading_the_week_still_counts_every_checkbox() {
    let vault = temp_vault("unfiltered");
    write_note(&vault);

    let week = run(&vault, &["week", "--date", WEDNESDAY]);
    let wed = day(&week, WEDNESDAY);
    assert_eq!(wed["openCount"], 3);
    assert_eq!(wed["doneCount"], 1);
}
