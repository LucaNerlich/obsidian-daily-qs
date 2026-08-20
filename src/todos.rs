//! Parse and mutate markdown checkbox todos in a daily note.

use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::NaiveDate;
use regex::Regex;

use crate::config::{DailyNotesConfig, Vault, VaultError};
use crate::open;
use crate::status::{Snapshot, TodoItem};

fn checkbox_re() -> Regex {
    Regex::new(r"^(\s*)([-*+])\s+\[([ xX])\](\s+)(.*)$").expect("checkbox regex")
}

fn tasks_heading_re() -> Regex {
    Regex::new(r"(?i)^#{1,6}\s+tasks\s*$").expect("tasks heading regex")
}

/// Parse all checkbox todos from note body. Line numbers are 1-based.
pub fn parse_todos(content: &str) -> Vec<TodoItem> {
    let re = checkbox_re();
    content
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let caps = re.captures(line)?;
            let checked = !caps[3].eq(" ");
            let text = caps[5].to_string();
            Some(TodoItem {
                line: idx + 1,
                checked,
                text,
            })
        })
        .collect()
}

pub fn read_snapshot(vault: &Vault, date: NaiveDate) -> Result<Snapshot, VaultError> {
    let config = vault.daily_notes_config()?;
    let path = vault.daily_note_path(&config, date)?;
    let date_str = date.format("%Y-%m-%d").to_string();
    let path_str = path.display().to_string();
    let todos = if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
        parse_todos(&content)
    } else {
        Vec::new()
    };
    let exists = path.exists();
    let mut snap = Snapshot::ok(date_str, path_str, exists, todos);
    enrich_snapshot(vault, date, &path, &mut snap)?;
    Ok(snap)
}

fn enrich_snapshot(
    vault: &Vault,
    date: NaiveDate,
    path: &Path,
    snap: &mut Snapshot,
) -> Result<(), VaultError> {
    snap.obsidian_uri = Some(open::open_uri(path));
    let today = chrono::Local::now().date_naive();
    snap.is_today = Some(date == today);
    let prev = date.checked_sub_days(chrono::Days::new(1)).unwrap_or(date);
    snap.carry_over_count = Some(open_todo_count(vault, prev)?);
    Ok(())
}

fn open_todo_count(vault: &Vault, date: NaiveDate) -> Result<usize, VaultError> {
    let config = vault.daily_notes_config()?;
    let path = vault.daily_note_path(&config, date)?;
    if !path.exists() {
        return Ok(0);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    Ok(parse_todos(&content).iter().filter(|t| !t.checked).count())
}

fn open_todo_texts(vault: &Vault, date: NaiveDate) -> Result<Vec<String>, VaultError> {
    let config = vault.daily_notes_config()?;
    let path = vault.daily_note_path(&config, date)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    Ok(parse_todos(&content)
        .into_iter()
        .filter(|t| !t.checked)
        .map(|t| t.text)
        .collect())
}

/// Copy yesterday's still-open todos into `date` (usually today).
pub fn carry_over(vault: &Vault, date: NaiveDate) -> Result<Snapshot, VaultError> {
    let prev = date
        .checked_sub_days(chrono::Days::new(1))
        .ok_or_else(|| VaultError::Io("date underflow".into()))?;
    let texts = open_todo_texts(vault, prev)?;
    if texts.is_empty() {
        return read_snapshot(vault, date);
    }
    let existing: std::collections::HashSet<String> = {
        let snap = read_snapshot(vault, date)?;
        snap.todos
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !t.checked)
            .map(|t| t.text)
            .collect()
    };
    for text in texts {
        if existing.contains(&text) {
            continue;
        }
        add_todo(vault, date, &text)?;
    }
    read_snapshot(vault, date)
}

/// Open the daily note in Obsidian via `xdg-open`.
pub fn open_in_obsidian(vault: &Vault, date: NaiveDate) -> Result<Snapshot, VaultError> {
    let snap = read_snapshot(vault, date)?;
    let uri = snap
        .obsidian_uri
        .clone()
        .ok_or_else(|| VaultError::Io("missing obsidian URI".into()))?;
    open::launch(&uri)?;
    Ok(snap)
}

/// Create today's note (and parents) if missing. Returns true when created.
pub fn ensure_note(
    vault: &Vault,
    config: &DailyNotesConfig,
    path: &Path,
    date: NaiveDate,
) -> Result<bool, VaultError> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| VaultError::Io(format!("failed to create {}: {e}", parent.display())))?;
    }
    let body = load_template_body(vault, config, date);
    write_atomic(path, &body)?;
    Ok(true)
}

fn load_template_body(vault: &Vault, config: &DailyNotesConfig, date: NaiveDate) -> String {
    if let Some(template_path) = vault.template_path(config) {
        if let Ok(raw) = fs::read_to_string(&template_path) {
            return expand_template(&raw, date);
        }
    }
    format!("# {}\n", date.format("%Y-%m-%d"))
}

fn expand_template(raw: &str, date: NaiveDate) -> String {
    // Minimal Obsidian template tokens used in daily note templates.
    raw.replace("{{date}}", &date.format("%Y-%m-%d").to_string())
        .replace("{{date:YYYY-MM-DD}}", &date.format("%Y-%m-%d").to_string())
        .replace("{{time}}", "")
}

/// Append a new open todo. Creates the note when missing.
pub fn add_todo(vault: &Vault, date: NaiveDate, text: &str) -> Result<Snapshot, VaultError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(VaultError::Io("todo text is empty".into()));
    }
    if text.contains('\n') || text.contains('\r') {
        return Err(VaultError::Io("todo text must be a single line".into()));
    }
    let config = vault.daily_notes_config()?;
    let path = vault.daily_note_path(&config, date)?;
    ensure_note(vault, &config, &path, date)?;
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    let next = insert_todo_line(&content, text);
    write_atomic(&path, &next)?;
    read_snapshot(vault, date)
}

/// Toggle the checkbox on the given 1-based line.
pub fn toggle_todo(vault: &Vault, date: NaiveDate, line: usize) -> Result<Snapshot, VaultError> {
    if line == 0 {
        return Err(VaultError::Io("line must be >= 1".into()));
    }
    let config = vault.daily_notes_config()?;
    let path = vault.daily_note_path(&config, date)?;
    if !path.exists() {
        return Err(VaultError::Io(format!(
            "daily note does not exist: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    let next = toggle_line(&content, line)?;
    write_atomic(&path, &next)?;
    read_snapshot(vault, date)
}

fn insert_todo_line(content: &str, text: &str) -> String {
    let item = format!("- [ ] {text}");
    let heading = tasks_heading_re();
    let lines: Vec<&str> = content.lines().collect();
    let mut insert_at: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if heading.is_match(line) {
            // Insert after the heading, skipping a single following blank line.
            let mut at = idx + 1;
            if at < lines.len() && lines[at].trim().is_empty() {
                at += 1;
            }
            insert_at = Some(at);
            break;
        }
    }

    let mut out: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();
    match insert_at {
        Some(at) => {
            out.insert(at, item);
        }
        None => {
            if !out.is_empty() && !out.last().map(|s| s.is_empty()).unwrap_or(true) {
                out.push(String::new());
            }
            out.push(item);
        }
    }
    let mut body = out.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body
}

fn toggle_line(content: &str, line: usize) -> Result<String, VaultError> {
    let re = checkbox_re();
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let idx = line - 1;
    if idx >= lines.len() {
        return Err(VaultError::Io(format!(
            "line {line} is past end of file ({} lines)",
            lines.len()
        )));
    }
    let current = lines[idx].clone();
    let caps = re
        .captures(&current)
        .ok_or_else(|| VaultError::Io(format!("line {line} is not a checkbox todo")))?;
    let checked = !caps[3].eq(" ");
    let mark = if checked { " " } else { "x" };
    lines[idx] = format!(
        "{}{} [{}]{}{}",
        &caps[1], &caps[2], mark, &caps[4], &caps[5]
    );
    let mut body = lines.join("\n");
    if content.ends_with('\n') && !body.ends_with('\n') {
        body.push('\n');
    }
    Ok(body)
}

fn write_atomic(path: &Path, content: &str) -> Result<(), VaultError> {
    let tmp = path.with_extension("md.tmp-obsidian-daily-qs");
    {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| VaultError::Io(format!("failed to create {}: {e}", tmp.display())))?;
        file.write_all(content.as_bytes())
            .map_err(|e| VaultError::Io(format!("failed to write {}: {e}", tmp.display())))?;
        file.sync_all()
            .map_err(|e| VaultError::Io(format!("failed to sync {}: {e}", tmp.display())))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        VaultError::Io(format!("failed to replace {}: {e}", path.display()))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DailyNotesConfig;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique_temp(prefix: &str) -> std::path::PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "obsidian-daily-qs-{prefix}-{}-{}-{}",
            std::process::id(),
            n,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn vault_with(content: &str) -> (Vault, NaiveDate, std::path::PathBuf) {
        let root = unique_temp("todos");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let note = root.join("Daily").join("2026-08-20.md");
        fs::create_dir_all(note.parent().unwrap()).unwrap();
        fs::write(&note, content).unwrap();
        (Vault { root }, date, note)
    }

    #[test]
    fn parses_mixed_checkboxes() {
        let todos = parse_todos(
            "# Day\n\n- [ ] open\n* [x] done\n+ [X] also\nnot a todo\n  - [ ] indented\n",
        );
        assert_eq!(todos.len(), 4);
        assert!(!todos[0].checked);
        assert_eq!(todos[0].line, 3);
        assert!(todos[1].checked);
        assert!(todos[2].checked);
        assert_eq!(todos[3].line, 7);
        assert_eq!(todos[3].text, "indented");
    }

    #[test]
    fn adds_under_tasks_heading() {
        let (vault, date, note) = vault_with("# Day\n\n## Tasks\n\n- [ ] existing\n\n## Notes\n");
        add_todo(&vault, date, "new one").unwrap();
        let body = fs::read_to_string(&note).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        let tasks_idx = lines.iter().position(|l| *l == "## Tasks").unwrap();
        assert_eq!(lines[tasks_idx + 2], "- [ ] new one");
        assert!(lines.contains(&"- [ ] existing"));
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn appends_when_no_tasks_heading() {
        let (vault, date, note) = vault_with("# Day\n\nhello\n");
        add_todo(&vault, date, "solo").unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.ends_with("- [ ] solo\n"));
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn toggles_checkbox() {
        let (vault, date, note) = vault_with("- [ ] open\n- [x] done\n");
        toggle_todo(&vault, date, 1).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.starts_with("- [x] open\n"));
        toggle_todo(&vault, date, 2).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.contains("- [ ] done\n"));
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn creates_note_from_template_on_add() {
        let root = unique_temp("create");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("Templates")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD","template":"Templates/Daily"}"#,
        )
        .unwrap();
        fs::write(
            root.join("Templates/Daily.md"),
            "# {{date:YYYY-MM-DD}}\n\n## Tasks\n\n",
        )
        .unwrap();
        let vault = Vault { root: root.clone() };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        add_todo(&vault, date, "first").unwrap();
        let note = root.join("Daily/2026-08-20.md");
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.contains("# 2026-08-20"));
        assert!(body.contains("## Tasks"));
        assert!(body.contains("- [ ] first"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ensure_note_is_noop_when_present() {
        let (vault, date, note) = vault_with("keep\n");
        let config = DailyNotesConfig {
            folder: "Daily".into(),
            format: "YYYY-MM-DD".into(),
            template: None,
        };
        assert!(!ensure_note(&vault, &config, &note, date).unwrap());
        assert_eq!(fs::read_to_string(&note).unwrap(), "keep\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_copies_open_from_previous_day() {
        let (vault, today, _) = vault_with("- [ ] today-only\n");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&ynote, "- [ ] leftover\n- [x] finished\n").unwrap();
        let snap = carry_over(&vault, today).unwrap();
        let texts: Vec<_> = snap.todos.unwrap().into_iter().map(|t| t.text).collect();
        assert!(texts.contains(&"leftover".to_string()));
        assert!(texts.contains(&"today-only".to_string()));
        assert!(!texts.iter().any(|t| t == "finished"));
        // Idempotent: second carry-over does not duplicate.
        let snap2 = carry_over(&vault, today).unwrap();
        assert_eq!(
            snap2
                .todos
                .unwrap()
                .iter()
                .filter(|t| t.text == "leftover")
                .count(),
            1
        );
        let _ = fs::remove_dir_all(vault.root());
    }
}
