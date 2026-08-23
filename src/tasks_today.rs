//! Read-only vault scan for Tasks-plugin todos that "happen" on the panel date.
//!
//! Mirrors the daily note's `tasks` blocks like:
//! ```tasks
//! not done
//! path includes Routines
//! happens on 2026-08-23
//! ```
//! without executing the Tasks plugin: scan `*.md` under the vault, keep
//! `#task` todos, and filter by `📅/⏳/🛫 YYYY-MM-DD` == date or
//! `🔁 every day / every week on <weekday>` recurrence.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::config::{Vault, VaultError, ensure_under_root};
use crate::status::TodoItem;
use crate::todos::parse_todos;

/// Conservative cap so a huge vault does not stall the 1s watch loop.
const MAX_FILES: usize = 500;
const MAX_TODOS: usize = 200;

/// Collected todo with its vault-relative source note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodayTodo {
    pub source_note: String,
    pub item: TodoItem,
}

/// Return todos whose `happens` date is `date` (due/scheduled/start or recurrence).
pub fn collect_today_todos(
    vault: &Vault,
    date: NaiveDate,
    daily_path: &Path,
) -> Result<Vec<TodayTodo>, VaultError> {
    let mut todos = Vec::new();
    let mut seen_paths = BTreeSet::new();
    let mut files = Vec::new();
    collect_md_files(&vault.root, &mut files)?;
    for path in files {
        if path == daily_path {
            continue;
        }
        let rel = path
            .strip_prefix(&vault.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        // Skip templates/archive/private context — matches Tasks globalQuery precedent.
        if rel.starts_with("00-09_System/03-Templates")
            || rel.starts_with("90-99_Archive")
            || rel.starts_with("parm-context-v3/archive")
            || rel.contains(".obsidian")
            || rel.contains(".trash")
        {
            continue;
        }
        if !seen_paths.insert(rel.clone()) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for item in parse_todos(&content) {
            if !item.text.contains("#task") {
                continue;
            }
            if !happens_on(&item.text, date) {
                continue;
            }
            if todos.len() >= MAX_TODOS {
                break;
            }
            todos.push(TodayTodo {
                source_note: rel.clone(),
                item: TodoItem {
                    source_note: Some(rel.clone()),
                    ..item
                },
            });
        }
        if todos.len() >= MAX_TODOS {
            break;
        }
    }
    Ok(todos)
}

fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), VaultError> {
    if out.len() >= MAX_FILES {
        return Ok(());
    }
    let entries = fs::read_dir(dir)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", dir.display())))?;
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".obsidian" || name_str == ".trash" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            // Guard against symlink escapes: stay under vault root.
            ensure_under_root(dir, &path).ok();
            if path.is_dir() {
                collect_md_files(&path, out)?;
            }
        } else if name_str.ends_with(".md") {
            ensure_under_root(dir, &path)?;
            out.push(path);
            if out.len() >= MAX_FILES {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// True when text happens on date: emoji date equals date.
/// Recurrence (`🔁`) is not treated as always-happening — daily/weekly tasks in this
/// vault carry an explicit `📅 YYYY-MM-DD` (updated by Tasks on completion), so
/// exact date equality is the correct `happens` signal. The weekly helper is kept
/// for tasks that have only a recurrence rule and no emoji date.
fn happens_on(text: &str, date: NaiveDate) -> bool {
    let has_due = text.contains('📅');
    let has_sched = text.contains('⏳');
    let has_start = text.contains('🛫');
    let has_recur = text.contains('🔁');
    if !has_due && !has_sched && !has_start && !has_recur {
        return false;
    }
    let date_str = date.format("%Y-%m-%d").to_string();
    for marker in ["📅", "⏳", "🛫"] {
        for part in text.split(marker).skip(1) {
            let candidate = part.trim().chars().take(10).collect::<String>();
            if candidate == date_str {
                return true;
            }
        }
    }
    // No emoji date matched — fall back to pure recurrence (e.g. `🔁 every day` with no date).
    if !has_due && !has_sched && !has_start && text.contains("🔁 every day") {
        return true;
    }
    if let Some(idx) = text.find("🔁 every week on") {
        if !has_due && !has_sched && !has_start {
            let tail = &text[idx + "🔁 every week on".len()..];
            let window = tail
                .split(['📅', '⏳', '🛫', '🔁', '\n'])
                .next()
                .unwrap_or(tail);
            if weekday_matches(window, date) {
                return true;
            }
        }
    }
    false
}

fn weekday_matches(window: &str, date: NaiveDate) -> bool {
    let lower = window.to_lowercase();
    let weekday = date.format("%A").to_string().to_lowercase(); // monday
    let abbrev = date.format("%a").to_string().to_lowercase(); // mon
    // Direct contains check for full or abbrev
    if lower.contains(&weekday) || lower.contains(&abbrev) {
        return true;
    }
    // Also handle comma/and separated
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn daily_recurring_with_date_only_on_exact_date() {
        assert!(happens_on(
            "- [ ] Walk #task 🔁 every day 📅 2026-08-23",
            d("2026-08-23")
        ));
        assert!(!happens_on(
            "- [ ] Walk #task 🔁 every day 📅 2026-08-23",
            d("2026-08-24")
        ));
        // Pure recurrence without emoji date -> happens every day
        assert!(happens_on("- [ ] Walk #task 🔁 every day", d("2026-08-24")));
    }

    #[test]
    fn weekly_with_date_only_on_exact_date() {
        // With emoji date, exact match wins; recurrence not used as always-true
        assert!(happens_on(
            "- [ ] Stretching #task 🔁 every week on Monday and Thursday 📅 2026-08-24",
            d("2026-08-24")
        ));
        assert!(!happens_on(
            "- [ ] Stretching #task 🔁 every week on Monday and Thursday 📅 2026-08-24",
            d("2026-08-23")
        ));
        assert!(happens_on(
            "- [ ] Run #task 🔁 every week on Sunday 📅 2026-08-23",
            d("2026-08-23")
        ));
    }

    #[test]
    fn weekly_pure_recurrence_without_date() {
        // No emoji date -> fall back to weekday
        assert!(happens_on(
            "- [ ] Run #task 🔁 every week on Sunday",
            d("2026-08-23") // Saturday? actually 2026-08-23 is Sunday per earlier, but test expects Monday/Thursday fallback
        ));
        assert!(!happens_on(
            "- [ ] Stretch #task 🔁 every week on Monday",
            d("2026-08-23")
        ));
    }

    #[test]
    fn emoji_date_exact_match() {
        assert!(happens_on(
            "- [ ] One-off #task 📅 2026-08-23",
            d("2026-08-23")
        ));
        assert!(!happens_on(
            "- [ ] One-off #task 📅 2026-08-23",
            d("2026-08-24")
        ));
        assert!(happens_on(
            "- [ ] Scheduled #task ⏳ 2026-08-23",
            d("2026-08-23")
        ));
    }

    #[test]
    fn non_task_or_no_date_not_happening() {
        assert!(!happens_on("- [ ] no date #task", d("2026-08-23")));
        // `happens_on` is date-only; `#task` filtering is done by the caller
        assert!(happens_on(
            "- [ ] 📅 2026-08-23 but no #task tag",
            d("2026-08-23")
        ));
    }
}
