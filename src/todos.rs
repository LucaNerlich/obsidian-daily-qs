//! Parse and mutate markdown checkbox todos in a daily note.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use regex::Regex;

use crate::config::{DailyNotesConfig, Vault, VaultError};
use crate::open;
use crate::status::{Snapshot, TodoItem};

fn checkbox_re() -> Regex {
    Regex::new(r"^(\s*)([-*+])\s+\[([ xX])\](\s+)(.*)$").expect("checkbox regex")
}

fn tasks_heading_re() -> Regex {
    // Accept both "Tasks" and "Todos" headings (case-insensitive).
    Regex::new(r"(?i)^#{1,6}\s+(tasks|todos)\s*$").expect("tasks heading regex")
}

/// Count indentation levels of a checkbox prefix: every tab is one level,
/// every two spaces one level (matches Obsidian's list indentation).
fn indent_level(indent: &str) -> usize {
    let tabs = indent.chars().filter(|c| *c == '\t').count();
    let spaces = indent.chars().filter(|c| *c == ' ').count();
    tabs + spaces / 2
}

/// Parse all checkbox todos from note body. Line numbers are 1-based.
///
/// Nested list items are normalized: each todo's `depth` is at most one level
/// deeper than the previous todo's, and `parent_line` points to the nearest
/// preceding todo at a shallower depth. An indented first todo has no parent
/// and is treated as top-level.
pub fn parse_todos(content: &str) -> Vec<TodoItem> {
    let re = checkbox_re();
    let mut todos = Vec::new();
    // Line numbers of open ancestors, indexed by depth (stack[d] has depth d).
    let mut stack: Vec<usize> = Vec::new();
    let mut last_depth = 0usize;
    for (idx, line) in content.lines().enumerate() {
        let Some(caps) = re.captures(line) else {
            continue;
        };
        let line_no = idx + 1;
        let checked = !caps[3].eq(" ");
        let text = caps[5].to_string();
        let raw = indent_level(&caps[1]);
        let depth = if stack.is_empty() {
            0
        } else {
            raw.min(last_depth + 1)
        };
        while stack.len() > depth {
            stack.pop();
        }
        let parent_line = stack.last().copied();
        stack.push(line_no);
        last_depth = depth;
        todos.push(TodoItem {
            line: line_no,
            checked,
            text,
            depth,
            parent_line,
        });
    }
    todos
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

struct OpenTodo {
    depth: usize,
    text: String,
}

fn open_todo_items(vault: &Vault, date: NaiveDate) -> Result<Vec<OpenTodo>, VaultError> {
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
        .map(|t| OpenTodo {
            depth: t.depth,
            text: t.text,
        })
        .collect())
}

/// Copy yesterday's still-open todos into `date` (usually today), preserving
/// their nesting. Todos whose parent was not carried are re-parented under
/// the nearest carried ancestor (depth is clamped to previous depth + 1).
pub fn carry_over(vault: &Vault, date: NaiveDate) -> Result<Snapshot, VaultError> {
    let prev = date
        .checked_sub_days(chrono::Days::new(1))
        .ok_or_else(|| VaultError::Io("date underflow".into()))?;
    let items = open_todo_items(vault, prev)?;
    if items.is_empty() {
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
    let to_add: Vec<(usize, String)> = items
        .into_iter()
        .filter(|item| !existing.contains(&item.text))
        .map(|item| (item.depth, item.text))
        .collect();
    if to_add.is_empty() {
        return read_snapshot(vault, date);
    }
    add_todo_lines(vault, date, &to_add)?;
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
    write_atomic(vault.root(), path, &body)?;
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

/// Append new open todos with the given nesting depths. Creates the note
/// when missing.
pub fn add_todo(vault: &Vault, date: NaiveDate, text: &str) -> Result<Snapshot, VaultError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(VaultError::Io("todo text is empty".into()));
    }
    if text.contains('\n') || text.contains('\r') {
        return Err(VaultError::Io("todo text must be a single line".into()));
    }
    add_todo_lines(vault, date, &[(0, text.to_string())])?;
    read_snapshot(vault, date)
}

/// Append the given `(depth, text)` todos as indented `- [ ] …` lines.
/// Depths are clamped so each line nests at most one level deeper than the
/// previous inserted line.
fn add_todo_lines(
    vault: &Vault,
    date: NaiveDate,
    items: &[(usize, String)],
) -> Result<(), VaultError> {
    let config = vault.daily_notes_config()?;
    let path = vault.daily_note_path(&config, date)?;
    ensure_note(vault, &config, &path, date)?;
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    let mut prev_depth: Option<usize> = None;
    let lines: Vec<String> = items
        .iter()
        .map(|(depth, text)| {
            // The first inserted line has no carried ancestor, so orphaned
            // children (parent checked / not carried) become top-level.
            let d = match prev_depth {
                None => 0,
                Some(p) => (*depth).min(p + 1),
            };
            prev_depth = Some(d);
            format!("{}- [ ] {}", "  ".repeat(d), text)
        })
        .collect();
    let next = insert_todo_lines(&content, &lines);
    write_atomic(vault.root(), &path, &next)
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
    write_atomic(vault.root(), &path, &next)?;
    read_snapshot(vault, date)
}

fn insert_todo_lines(content: &str, items: &[String]) -> String {
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
            for (offset, item) in items.iter().enumerate() {
                out.insert(at + offset, item.clone());
            }
        }
        None => {
            if !out.is_empty() && !out.last().map(|s| s.is_empty()).unwrap_or(true) {
                out.push(String::new());
            }
            out.extend(items.iter().cloned());
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

/// Resolve the note's real location and verify it stays inside the vault.
///
/// The parent directory is canonicalized so a symlinked daily-notes folder is
/// followed to its real location; anything outside the (canonicalized) vault
/// root is refused. An existing note may itself be a symlink — if its target
/// lies inside the vault it is written through, otherwise the write is
/// refused.
fn resolve_note_target(vault_root: &Path, path: &Path) -> Result<PathBuf, VaultError> {
    let root = vault_root.canonicalize().map_err(|e| {
        VaultError::Io(format!(
            "failed to resolve vault root {}: {e}",
            vault_root.display()
        ))
    })?;
    let parent = path
        .parent()
        .ok_or_else(|| VaultError::Io(format!("note path has no parent: {}", path.display())))?;
    let real_parent = parent
        .canonicalize()
        .map_err(|e| VaultError::Io(format!("failed to resolve {}: {e}", parent.display())))?;
    if !real_parent.starts_with(&root) {
        return Err(VaultError::Io(format!(
            "refusing to write outside the vault: {} resolves to {}",
            parent.display(),
            real_parent.display()
        )));
    }
    let name = path
        .file_name()
        .ok_or_else(|| VaultError::Io(format!("note path has no file name: {}", path.display())))?;
    let resolved = real_parent.join(name);
    // A missing note is created at the already-verified parent; a note that
    // resolves through symlinks must end up inside the vault.
    match fs::canonicalize(&resolved) {
        Ok(real) if real.starts_with(&root) => Ok(real),
        Ok(real) => Err(VaultError::Io(format!(
            "refusing to write outside the vault: {} resolves to {}",
            path.display(),
            real.display()
        ))),
        Err(_) => Ok(resolved),
    }
}

/// Unpredictable sibling of `target`: same directory so the final rename
/// stays atomic, random suffix so a pre-created path cannot collide.
fn temp_path_for(target: &Path) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut name = target
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(
        ".tmp-obsidian-daily-qs-{}-{nonce}",
        std::process::id()
    ));
    target.with_file_name(name)
}

fn write_atomic(vault_root: &Path, path: &Path, content: &str) -> Result<(), VaultError> {
    let target = resolve_note_target(vault_root, path)?;
    let tmp = temp_path_for(&target);
    {
        // `create_new` is O_CREAT|O_EXCL: it fails if anything — including a
        // symlink — already sits at the temp path, so a pre-created link
        // cannot redirect this write. The parent was canonicalized above, so
        // the temp file cannot escape through a symlinked directory either.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| VaultError::Io(format!("failed to create {}: {e}", tmp.display())))?;
        file.write_all(content.as_bytes())
            .map_err(|e| VaultError::Io(format!("failed to write {}: {e}", tmp.display())))?;
        file.sync_all()
            .map_err(|e| VaultError::Io(format!("failed to sync {}: {e}", tmp.display())))?;
    }
    fs::rename(&tmp, &target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        VaultError::Io(format!("failed to replace {}: {e}", target.display()))
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
    fn parses_nested_todos() {
        let todos = parse_todos(
            "- [ ] parent\n\t- [ ] tab-child\n  - [ ] space-child\n    - [ ] grand-child\n- [ ] sibling\n",
        );
        assert_eq!(todos.len(), 5);
        assert_eq!(todos[0].depth, 0);
        assert_eq!(todos[0].parent_line, None);
        // Tab and two spaces are both one level.
        assert_eq!(todos[1].depth, 1);
        assert_eq!(todos[1].parent_line, Some(todos[0].line));
        assert_eq!(todos[2].depth, 1);
        assert_eq!(todos[2].parent_line, Some(todos[0].line));
        assert_eq!(todos[3].depth, 2);
        assert_eq!(todos[3].parent_line, Some(todos[2].line));
        assert_eq!(todos[4].depth, 0);
        assert_eq!(todos[4].parent_line, None);
    }

    #[test]
    fn normalizes_deep_indentation_and_lone_nested_todos() {
        // A todo can only nest one level deeper than the previous one, and an
        // indented first todo has no parent, so it becomes top-level.
        let todos = parse_todos("      - [ ] orphan\n- [ ] parent\n        - [ ] child\n");
        assert_eq!(todos[0].depth, 0);
        assert_eq!(todos[0].parent_line, None);
        assert_eq!(todos[1].depth, 0);
        assert_eq!(todos[2].depth, 1);
        assert_eq!(todos[2].parent_line, Some(todos[1].line));
    }

    #[test]
    fn adds_under_todos_heading() {
        let (vault, date, note) = vault_with("# Day\n\n## Todos\n\n- [ ] existing\n\n## Notes\n");
        add_todo(&vault, date, "new one").unwrap();
        let body = fs::read_to_string(&note).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        let todos_idx = lines.iter().position(|l| *l == "## Todos").unwrap();
        assert_eq!(lines[todos_idx + 2], "- [ ] new one");
        assert!(lines.contains(&"- [ ] existing"));
        let _ = fs::remove_dir_all(vault.root());
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
    fn carry_over_preserves_nesting() {
        let (vault, today, _) = vault_with("- [ ] keep\n");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(
            &ynote,
            "- [ ] parent\n  - [ ] child\n  - [x] done-child\n- [x] finished\n- [ ] solo\n",
        )
        .unwrap();
        let snap = carry_over(&vault, today).unwrap();
        let todos = snap.todos.unwrap();
        assert_eq!(todos.len(), 4);
        assert_eq!(todos[0].text, "keep");
        assert_eq!(todos[1].text, "parent");
        assert_eq!(todos[1].depth, 0);
        assert_eq!(todos[2].text, "child");
        assert_eq!(todos[2].depth, 1);
        assert_eq!(todos[2].parent_line, Some(todos[1].line));
        assert_eq!(todos[3].text, "solo");
        assert_eq!(todos[3].depth, 0);
        let note = vault.root().join("Daily/2026-08-20.md");
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.contains("- [ ] parent\n  - [ ] child\n- [ ] solo\n"));
        assert!(!body.contains("done-child"));
        assert!(!body.contains("finished"));
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_reparents_orphaned_children() {
        // Child of a checked parent: the parent is not carried, so the child
        // is clamped to one level under the previous carried todo.
        let (vault, today, _) = vault_with("");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&ynote, "- [x] parent\n    - [ ] deep-child\n").unwrap();
        let snap = carry_over(&vault, today).unwrap();
        let todos = snap.todos.unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].text, "deep-child");
        assert_eq!(todos[0].depth, 0);
        let _ = fs::remove_dir_all(vault.root());
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

    #[cfg(unix)]
    #[test]
    fn refuses_write_through_symlinked_note_folder() {
        use std::os::unix::fs::symlink;
        let (vault, date, _note) = vault_with("- [ ] open\n");
        let outside = unique_temp("outside-folder");
        fs::create_dir_all(&outside).unwrap();
        let daily = vault.root().join("Daily");
        fs::remove_dir_all(&daily).unwrap();
        symlink(&outside, &daily).unwrap();
        let err = add_todo(&vault, date, "nope").unwrap_err();
        assert!(err.to_string().contains("outside the vault"), "err: {err}");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        let _ = fs::remove_dir_all(vault.root());
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_write_when_note_symlinks_outside() {
        use std::os::unix::fs::symlink;
        let (vault, date, note) = vault_with("- [ ] open\n");
        let outside = unique_temp("outside-note");
        fs::create_dir_all(&outside).unwrap();
        let real = outside.join("target.md");
        fs::write(&real, "- [ ] precious\n").unwrap();
        fs::remove_file(&note).unwrap();
        symlink(&real, &note).unwrap();
        let err = toggle_todo(&vault, date, 1).unwrap_err();
        // The daily-note path check resolves the symlink and refuses before
        // any write is attempted.
        assert!(err.to_string().contains("escapes vault root"), "err: {err}");
        assert_eq!(fs::read_to_string(&real).unwrap(), "- [ ] precious\n");
        let _ = fs::remove_dir_all(vault.root());
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn follows_in_vault_symlinked_note() {
        use std::os::unix::fs::symlink;
        let (vault, date, note) = vault_with("- [ ] open\n");
        let archive = vault.root().join("Archive");
        fs::create_dir_all(&archive).unwrap();
        let real = archive.join("2026-08-20.md");
        fs::write(&real, "- [ ] open\n").unwrap();
        fs::remove_file(&note).unwrap();
        symlink(&real, &note).unwrap();
        toggle_todo(&vault, date, 1).unwrap();
        // The real in-vault file is updated and the user's symlink survives.
        assert!(fs::read_to_string(&real).unwrap().starts_with("- [x] open"));
        assert!(
            fs::symlink_metadata(&note)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[cfg(unix)]
    #[test]
    fn precreated_temp_symlink_cannot_redirect() {
        use std::os::unix::fs::symlink;
        let (vault, date, note) = vault_with("- [ ] open\n");
        let outside = unique_temp("outside-tmp");
        fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("victim.txt");
        fs::write(&victim, "untouched\n").unwrap();
        // The old predictable temp path, pre-created as a symlink to a file
        // outside the vault. The randomized temp name never collides with it.
        let stale = note.with_extension("md.tmp-obsidian-daily-qs");
        symlink(&victim, &stale).unwrap();
        toggle_todo(&vault, date, 1).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched\n");
        assert!(
            fs::symlink_metadata(&stale)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&note).unwrap().starts_with("- [x] open"));
        let _ = fs::remove_dir_all(vault.root());
        let _ = fs::remove_dir_all(outside);
    }
}
