//! Parse and mutate markdown checkbox todos in a daily note.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

use crate::config::{DailyNotesConfig, Vault, VaultError};
use crate::open;
use crate::status::{Snapshot, TodoItem};

// Compiled once per process. These are used on every parse and on every
// candidate day of the carry-over/style lookbacks, so rebuilding them per call
// meant dozens of compilations per poll of the always-on bar widget.

static CHECKBOX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)([-*+])\s+\[([ xX])\](\s+)(.*)$").expect("checkbox regex"));

static TASKS_HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Accept both "Tasks" and "Todos" headings (case-insensitive).
    Regex::new(r"(?i)^#{1,6}\s+(tasks|todos)\s*$").expect("tasks heading regex")
});

/// Matcher for markdown ATX headings (up to 3 leading spaces, optional closing
/// hashes). Shared by the heading filter and the insert path so both agree on
/// which lines delimit a section — previously the read side had its own,
/// stricter pattern, so `  ## Tasks` or `# Tasks #` was a section to insert
/// into but not one to display.
static ANY_HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ {0,3}(#{1,6})[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$").expect("heading regex")
});

fn checkbox_re() -> &'static Regex {
    &CHECKBOX_RE
}

fn tasks_heading_re() -> &'static Regex {
    &TASKS_HEADING_RE
}

fn any_heading_re() -> &'static Regex {
    &ANY_HEADING_RE
}

#[derive(Default)]
struct FenceScanner {
    open: Option<(u8, usize)>,
}

impl FenceScanner {
    /// Returns `true` for an opening/closing fence or a line inside one.
    fn is_fenced(&mut self, line: &str) -> bool {
        let bytes = line.as_bytes();
        let indent = bytes.iter().take_while(|&&byte| byte == b' ').count();
        if indent > 3 {
            return self.open.is_some();
        }

        let Some(&marker) = bytes.get(indent) else {
            return self.open.is_some();
        };
        if marker != b'`' && marker != b'~' {
            return self.open.is_some();
        }
        let length = bytes[indent..]
            .iter()
            .take_while(|&&byte| byte == marker)
            .count();

        if let Some((open_marker, open_length)) = self.open {
            if marker == open_marker
                && length >= open_length
                && bytes[indent + length..].iter().all(u8::is_ascii_whitespace)
            {
                self.open = None;
            }
            return true;
        }

        if length < 3 || (marker == b'`' && bytes[indent + length..].contains(&b'`')) {
            return false;
        }
        self.open = Some((marker, length));
        true
    }
}

/// Lines that are a fence marker or sit inside a fenced code block.
///
/// A checkbox-looking line in a code sample is not a todo. The read path, the
/// heading filter and the insert placement all consult this mask so they agree
/// on where the note's real todos are; otherwise the widget lists code lines,
/// counts them, and writes new todos inside the fence.
fn fence_mask(lines: &[&str]) -> Vec<bool> {
    let mut scanner = FenceScanner::default();
    lines.iter().map(|line| scanner.is_fenced(line)).collect()
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
    let lines: Vec<&str> = content.lines().collect();
    let fenced = fence_mask(&lines);
    let mut todos = Vec::new();
    // Line numbers of open ancestors, indexed by depth (stack[d] has depth d).
    let mut stack: Vec<usize> = Vec::new();
    let mut last_depth = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        if fenced[idx] {
            continue;
        }
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
    read_snapshot_filtered(vault, date, None)
}

/// Resolve the path of `date`'s note: the live daily-notes folder wins, but a
/// note manually moved into the archive folder configured via
/// `--archive-folder` / the `archiveFolder` bar setting is still found.
fn resolved_note_path(
    vault: &Vault,
    config: &DailyNotesConfig,
    date: NaiveDate,
) -> Result<PathBuf, VaultError> {
    Ok(vault.daily_note_paths(config, date)?.resolved)
}

/// Like [`read_snapshot`], but when `heading` is set only todos under a
/// matching markdown heading (until the next same-or-higher heading) are
/// included.
pub fn read_snapshot_filtered(
    vault: &Vault,
    date: NaiveDate,
    heading: Option<&str>,
) -> Result<Snapshot, VaultError> {
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    let date_str = date.format("%Y-%m-%d").to_string();
    let path_str = path.display().to_string();
    let todos = if path.exists() {
        let content = fs::read_to_string(&path)
            .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
        let all = parse_todos(&content);
        filter_todos_by_heading(&content, all, heading)
    } else {
        Vec::new()
    };
    let exists = path.exists();
    let mut snap = Snapshot::ok(date_str, path_str, exists, todos);
    enrich_snapshot(vault, date, &path, &config, &mut snap)?;
    Ok(snap)
}

fn filter_todos_by_heading(
    content: &str,
    todos: Vec<TodoItem>,
    heading: Option<&str>,
) -> Vec<TodoItem> {
    let Some(want) = heading.map(str::trim).filter(|s| !s.is_empty()) else {
        return todos;
    };
    let want_l = want.to_lowercase();
    let lines: Vec<&str> = content.lines().collect();
    let fenced = fence_mask(&lines);
    let heading_re = any_heading_re();
    // Inclusive start / exclusive end as 1-based file line numbers.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if !fenced[i]
            && let Some(caps) = heading_re.captures(lines[i])
        {
            let level = caps[1].len();
            let title = caps[2].trim().to_lowercase();
            if title == want_l {
                // Content under heading starts at the next line (1-based: i+2).
                let start = i + 2;
                let mut j = i + 1;
                while j < lines.len() {
                    if !fenced[j]
                        && let Some(next) = heading_re.captures(lines[j])
                        && next[1].len() <= level
                    {
                        break;
                    }
                    j += 1;
                }
                // j is 0-based exclusive end index → 1-based exclusive = j+1
                ranges.push((start, j + 1));
            }
        }
        i += 1;
    }
    if ranges.is_empty() {
        return Vec::new();
    }
    todos
        .into_iter()
        .filter(|t| ranges.iter().any(|(a, b)| t.line >= *a && t.line < *b))
        .collect()
}

fn enrich_snapshot(
    vault: &Vault,
    date: NaiveDate,
    path: &Path,
    config: &DailyNotesConfig,
    snap: &mut Snapshot,
) -> Result<(), VaultError> {
    snap.obsidian_uri = Some(open::open_uri(vault.root(), path));
    let today = chrono::Local::now().date_naive();
    snap.is_today = Some(date == today);
    snap.carry_over_count = Some(carry_source_count(vault, config, date)?.unwrap_or(0));
    if let Some(name) = config.template.clone() {
        snap.template_name = Some(name);
        let has_template = vault.template_path(config).is_some_and(|p| p.exists());
        snap.created_from_template = Some(has_template && path.exists());
    }
    Ok(())
}

/// How far back carry-over looks for the most recent previous daily note with
/// open todos. Covers weekend/vacation gaps without scanning forever; each
/// candidate is one small file read.
const CARRY_OVER_LOOKBACK_DAYS: u64 = 30;

/// Open count of the most recent previous day before `date` that still holds
/// open todos (the "last found" daily note), or `None` when no such note
/// exists in the lookback window.
fn carry_source(
    vault: &Vault,
    config: &DailyNotesConfig,
    date: NaiveDate,
) -> Result<Option<(NaiveDate, usize)>, VaultError> {
    for offset in 1..=CARRY_OVER_LOOKBACK_DAYS {
        let Some(prev) = date.checked_sub_days(chrono::Days::new(offset)) else {
            break;
        };
        let count = open_todo_count(vault, config, prev)?;
        if count > 0 {
            return Ok(Some((prev, count)));
        }
    }
    Ok(None)
}

fn carry_source_count(
    vault: &Vault,
    config: &DailyNotesConfig,
    date: NaiveDate,
) -> Result<Option<usize>, VaultError> {
    Ok(carry_source(vault, config, date)?.map(|(_, count)| count))
}

fn open_todo_count(
    vault: &Vault,
    config: &DailyNotesConfig,
    date: NaiveDate,
) -> Result<usize, VaultError> {
    let path = resolved_note_path(vault, config, date)?;
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

fn open_todo_items(
    vault: &Vault,
    config: &DailyNotesConfig,
    date: NaiveDate,
) -> Result<Vec<OpenTodo>, VaultError> {
    let path = resolved_note_path(vault, config, date)?;
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

/// Move still-open todos from the most recent previous daily note with open
/// todos ("last found", up to `CARRY_OVER_LOOKBACK_DAYS` back) into `date`
/// (usually today), preserving their nesting, and remove them from the source
/// note so it is left with only its done todos. Todos whose parent was not
/// carried are re-parented under the nearest carried ancestor (depth is
/// clamped to previous depth + 1).
/// Open todos that already exist in `date` are not duplicated. The source note
/// is emptied of at most as many lines per text as the target holds — enough to
/// complete a move that was interrupted between the append and the removal,
/// without deleting more identical-looking lines than the target can account
/// for. Returns the number of moved todos.
///
/// When `heading` names a markdown section (e.g. `Inbox`), carried items are
/// inserted there; otherwise the default placement (Tasks/Todos, first list,
/// after title) applies.
fn move_open_from_previous(
    vault: &Vault,
    date: NaiveDate,
    heading: Option<&str>,
) -> Result<usize, VaultError> {
    // Create the target note up front without rolling over: otherwise
    // add_todo_lines's ensure_note would create it here and re-enter this
    // function. That nested run finished completely (appending the items and
    // emptying the source note) before the outer add_todo_lines appended
    // the same lines again — duplicating every carried todo.
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    create_note_if_missing(vault, &config, &path, date)?;
    let Some((prev, _)) = carry_source(vault, &config, date)? else {
        return Ok(0);
    };
    let items = open_todo_items(vault, &config, prev)?;
    if items.is_empty() {
        return Ok(0);
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
    let to_move: Vec<(usize, String)> = items
        .iter()
        .filter(|item| !existing.contains(&item.text))
        .map(|item| (item.depth, item.text.clone()))
        .collect();

    // Read both notes' pre-mutation contents before touching anything: a read
    // failure must leave the vault untouched, and the undo record needs both
    // bodies to restore them together.
    let prev_path = resolved_note_path(vault, &config, prev)?;
    let prev_before = fs::read_to_string(&prev_path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", prev_path.display())))?;
    let target_before = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;

    if !to_move.is_empty()
        && let Err(e) = add_todo_lines(vault, date, &to_move, heading)
    {
        // The append is atomic, so nothing changed and there is nothing to undo.
        crate::undo::discard();
        return Err(e);
    }
    // Record both notes *before* the source is touched. The append above
    // recorded the target alone, so a single undo used to empty the target while
    // the source had already been emptied — the todos ended up in neither note.
    // Writing the pair here covers the success path and both interruption
    // points: a crash between this record and the removal, and a removal that
    // fails, both leave a record that restores the vault, so the removal failure
    // below must not discard it.
    crate::undo::record_before_multi(
        vault,
        date,
        &[
            (&prev_path, Some(prev_before.as_str())),
            (&path, Some(target_before.as_str())),
        ],
    )?;
    // Reconcile against the target's current open todos instead of only the
    // lines added above: if an earlier run added to the target but failed
    // before removing from the previous note (crash, write error), the
    // stranded items are still open in both notes and complete here.
    let target_open: Vec<(usize, String)> = {
        let snap = read_snapshot(vault, date)?;
        snap.todos
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !t.checked)
            .map(|t| (t.depth, t.text))
            .collect()
    };
    // The record above is deliberately kept on failure: the vault is then
    // half-moved (the target holds the carried copies and the source still does
    // too), and one undo is the repair. Discarding it — as an earlier draft did
    // — removed the only way back and reported "nothing to undo" exactly when it
    // was needed.
    let budget = removal_budget(
        items.iter().map(|item| item.text.as_str()),
        target_open.iter().map(|(_, text)| text.as_str()),
    );
    remove_todos(vault, prev, &budget)?;
    Ok(to_move.len())
}

/// How many source lines may be removed per todo text: the target's count for
/// that text, capped by how many open lines the source actually has. Each
/// target line can account for at most one source line.
fn removal_budget<'a>(
    source: impl Iterator<Item = &'a str>,
    target: impl Iterator<Item = &'a str>,
) -> std::collections::HashMap<String, usize> {
    let mut available: std::collections::HashMap<&'a str, usize> = std::collections::HashMap::new();
    for text in source {
        *available.entry(text).or_default() += 1;
    }
    let mut budget: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for text in target {
        let left = available.get(text).copied().unwrap_or(0);
        if left == 0 {
            continue;
        }
        let entry = budget.entry(text.to_string()).or_insert(0);
        if *entry < left {
            *entry += 1;
        }
    }
    budget
}

/// Remove still-open todos from `date`'s note, at most as many lines per text
/// as `budget` allows. The budget bounds the reconcile: a text collision with a
/// todo the target already had must not delete every identical-looking line in
/// the source note.
fn remove_todos(
    vault: &Vault,
    date: NaiveDate,
    budget: &std::collections::HashMap<String, usize>,
) -> Result<(), VaultError> {
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    let mut remaining = budget.clone();
    // Walk the candidates from the last line to the first, so the budget is
    // spent on the deepest lines: when two lines share both text and nesting,
    // the outermost one survives with its children instead of the child being
    // left orphaned (or re-parented under an unrelated checked item).
    let drop: std::collections::HashSet<usize> = parse_todos(&content)
        .into_iter()
        .rev()
        .filter(|t| !t.checked)
        .filter(|t| match remaining.get_mut(&t.text) {
            Some(left) if *left > 0 => {
                *left -= 1;
                true
            }
            _ => false,
        })
        .map(|t| t.line)
        .collect();
    if drop.is_empty() {
        return Ok(());
    }
    let next = without_lines(&content, &drop);
    write_atomic(vault.root(), &path, &next)
}

/// Roll the most recent previous daily note's open todos into `date`
/// (usually today): they are moved, so the source note keeps only its done
/// todos. When `heading` is set, carried items are inserted under that
/// markdown heading.
pub fn carry_over(
    vault: &Vault,
    date: NaiveDate,
    heading: Option<&str>,
) -> Result<Snapshot, VaultError> {
    move_open_from_previous(vault, date, heading)?;
    read_snapshot(vault, date)
}

/// Open the daily note in Obsidian via `xdg-open`.
pub fn open_in_obsidian(vault: &Vault, date: NaiveDate) -> Result<Snapshot, VaultError> {
    let snap = prepare_note_for_open(vault, date)?;
    let uri = snap
        .obsidian_uri
        .clone()
        .ok_or_else(|| VaultError::Io("missing obsidian URI".into()))?;
    open::launch(&uri)?;
    Ok(snap)
}

/// Create a missing note from the configured daily-note template before it is
/// opened. This deliberately skips carry-over: opening a future date must not
/// move unfinished todos out of the previous day.
fn prepare_note_for_open(vault: &Vault, date: NaiveDate) -> Result<Snapshot, VaultError> {
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    create_note_if_missing(vault, &config, &path, date)?;
    read_snapshot(vault, date)
}

/// Create the note (and parents) if missing, without rolling over. Returns
/// true when created.
fn create_note_if_missing(
    vault: &Vault,
    config: &DailyNotesConfig,
    path: &Path,
    date: NaiveDate,
) -> Result<bool, VaultError> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        create_dir_all_in_vault(vault.root(), parent)?;
    }
    let body = load_template_body(vault, config, date);
    write_atomic(vault.root(), path, &body)?;
    Ok(true)
}

/// Create today's note (and parents) if missing. Returns true when created.
///
/// When the note is created, the most recent previous daily note's open todos
/// are rolled over into it under `heading` (same placement as
/// `add`/`carry-over`).
pub fn ensure_note(
    vault: &Vault,
    config: &DailyNotesConfig,
    path: &Path,
    date: NaiveDate,
    heading: Option<&str>,
) -> Result<bool, VaultError> {
    let created = create_note_if_missing(vault, config, path, date)?;
    // First access of a new day: pull the most recent previous open todos
    // into the fresh note and leave the source with only its done todos.
    // Plain note creation carries no rollover, so this cannot re-enter itself.
    if created {
        move_open_from_previous(vault, date, heading)?;
    }
    Ok(created)
}

fn load_template_body(vault: &Vault, config: &DailyNotesConfig, date: NaiveDate) -> String {
    if let Some(template_path) = vault.template_path(config)
        && let Ok(raw) = fs::read_to_string(&template_path)
    {
        return expand_template(&raw, date);
    }
    format!("# {}\n", date.format("%Y-%m-%d"))
}

fn expand_template(raw: &str, date: NaiveDate) -> String {
    // Minimal Obsidian template tokens used in daily note templates.
    raw.replace("{{date}}", &date.format("%Y-%m-%d").to_string())
        .replace("{{date:YYYY-MM-DD}}", &date.format("%Y-%m-%d").to_string())
        .replace("{{time}}", "")
}

/// Append a new open todo. When `under_line` is set, nest one level under
/// that todo and insert immediately after it (the `heading` is ignored in
/// that case). Otherwise the todo is inserted under `heading` when it names
/// an existing markdown section, falling back to the default placement.
pub fn add_todo(
    vault: &Vault,
    date: NaiveDate,
    text: &str,
    heading: Option<&str>,
) -> Result<Snapshot, VaultError> {
    add_todo_under(vault, date, text, None, heading)
}

pub fn add_todo_under(
    vault: &Vault,
    date: NaiveDate,
    text: &str,
    under_line: Option<usize>,
    heading: Option<&str>,
) -> Result<Snapshot, VaultError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(VaultError::Io("todo text is empty".into()));
    }
    if text.contains('\n') || text.contains('\r') {
        return Err(VaultError::Io("todo text must be a single line".into()));
    }
    match under_line {
        None => {
            add_todo_lines(vault, date, &[(0, text.to_string())], heading)?;
        }
        Some(parent_line) => {
            if parent_line == 0 {
                return Err(VaultError::Io("under-line must be >= 1".into()));
            }
            let config = vault.daily_notes_config()?;
            let path = resolved_note_path(vault, &config, date)?;
            ensure_note(vault, &config, &path, date, heading)?;
            let content = fs::read_to_string(&path)
                .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
            let todos = parse_todos(&content);
            let parent = todos
                .iter()
                .find(|t| t.line == parent_line)
                .ok_or_else(|| VaultError::Io(format!("under-line {parent_line} is not a todo")))?;
            let depth = parent.depth + 1;
            let insert_at = parent_line; // insert after this 1-based line
            let new_line = format!("{}- [ ] {}", "  ".repeat(depth), text);
            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            lines.insert(insert_at, new_line);
            let next = join_body(&content, &lines);
            write_atomic_with_undo(vault, date, &path, &content, &next)?;
        }
    }
    read_snapshot(vault, date)
}

/// How checkbox lists are laid out in this vault's recent daily notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TodoStyle {
    /// No blank line between sibling todos.
    compact: bool,
    /// One blank line between a heading and the first todo.
    blank_after_heading: bool,
}

const DEFAULT_TODO_STYLE: TodoStyle = TodoStyle {
    compact: true,
    blank_after_heading: true,
};

/// Look back this many calendar days when the current note has no list yet.
const TODO_STYLE_LOOKBACK_DAYS: u64 = 14;

/// Append the given `(depth, text)` todos as indented `- [ ] …` lines.
/// Depths are clamped so each line nests at most one level deeper than the
/// previous inserted line. When `heading` names an existing markdown section,
/// items are inserted there; otherwise the default placement applies.
fn add_todo_lines(
    vault: &Vault,
    date: NaiveDate,
    items: &[(usize, String)],
    heading: Option<&str>,
) -> Result<(), VaultError> {
    let formatted: Vec<(usize, bool, String)> = items
        .iter()
        .map(|(depth, text)| (*depth, false, text.clone()))
        .collect();
    add_checkbox_lines(vault, date, &formatted, true, heading)
}

fn format_checkbox_lines(items: &[(usize, bool, String)]) -> Vec<String> {
    let mut prev_depth: Option<usize> = None;
    items
        .iter()
        .map(|(depth, checked, text)| {
            let d = match prev_depth {
                None => 0,
                Some(p) => (*depth).min(p + 1),
            };
            prev_depth = Some(d);
            let mark = if *checked { "x" } else { " " };
            format!("{}- [{mark}] {text}", "  ".repeat(d))
        })
        .collect()
}

fn add_checkbox_lines(
    vault: &Vault,
    date: NaiveDate,
    items: &[(usize, bool, String)],
    rollover: bool,
    heading: Option<&str>,
) -> Result<(), VaultError> {
    let config = vault.daily_notes_config()?;
    // Resolve first so archived notes are edited in place instead of
    // spawning a live duplicate when the live path is missing.
    let path = resolved_note_path(vault, &config, date)?;
    if rollover {
        ensure_note(vault, &config, &path, date, heading)?;
    } else {
        create_note_if_missing(vault, &config, &path, date)?;
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    let lines = format_checkbox_lines(items);
    let style = infer_todo_style(vault, &config, date, &content);
    let next = insert_todo_lines(&content, &lines, style, heading);
    if rollover {
        write_atomic_with_undo(vault, date, &path, &content, &next)
    } else {
        write_atomic(vault.root(), &path, &next)
    }
}

fn expect_line_text(
    content: &str,
    line: usize,
    expect_text: Option<&str>,
) -> Result<(), VaultError> {
    let Some(expected) = expect_text.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(());
    };
    let at_line = parse_todos(content)
        .into_iter()
        .find(|t| t.line == line)
        .map(|t| t.text);
    match at_line {
        Some(actual) if actual.trim() == expected => Ok(()),
        Some(actual) => Err(VaultError::Io(format!(
            "stale view: line {line} now holds {:?} instead of {:?}; refetch and retry",
            actual.trim(),
            expected
        ))),
        None => Err(VaultError::Io(format!(
            "stale view: line {line} is no longer a todo; refetch and retry"
        ))),
    }
}

/// Toggle the checkbox on the given 1-based line.
pub fn toggle_todo(
    vault: &Vault,
    date: NaiveDate,
    line: usize,
    expect_text: Option<&str>,
) -> Result<Snapshot, VaultError> {
    if line == 0 {
        return Err(VaultError::Io("line must be >= 1".into()));
    }
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    if !path.exists() {
        return Err(VaultError::Io(format!(
            "daily note does not exist: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    expect_line_text(&content, line, expect_text)?;
    let next = toggle_line(&content, line)?;
    write_atomic_with_undo(vault, date, &path, &content, &next)?;
    read_snapshot(vault, date)
}

pub fn edit_todo(
    vault: &Vault,
    date: NaiveDate,
    line: usize,
    expect_text: Option<&str>,
    new_text: &str,
) -> Result<Snapshot, VaultError> {
    if line == 0 {
        return Err(VaultError::Io("line must be >= 1".into()));
    }
    let new_text = new_text.trim();
    if new_text.is_empty() {
        return Err(VaultError::Io("todo text is empty".into()));
    }
    if new_text.contains('\n') || new_text.contains('\r') {
        return Err(VaultError::Io("todo text must be a single line".into()));
    }
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    if !path.exists() {
        return Err(VaultError::Io(format!(
            "daily note does not exist: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    expect_line_text(&content, line, expect_text)?;
    let re = checkbox_re();
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let idx = line - 1;
    if idx >= lines.len() {
        return Err(VaultError::Io(format!("line {line} is past end of file")));
    }
    reject_fenced_line(&content, line)?;
    let caps = re
        .captures(&lines[idx])
        .ok_or_else(|| VaultError::Io(format!("line {line} is not a checkbox todo")))?;
    let new_line = format!(
        "{}{} [{}]{}{}",
        &caps[1], &caps[2], &caps[3], &caps[4], new_text
    );
    let next = rewrite_line(&content, line, &new_line);
    write_atomic_with_undo(vault, date, &path, &content, &next)?;
    read_snapshot(vault, date)
}

pub fn delete_todo(
    vault: &Vault,
    date: NaiveDate,
    line: usize,
    expect_text: Option<&str>,
    with_children: bool,
) -> Result<Snapshot, VaultError> {
    if line == 0 {
        return Err(VaultError::Io("line must be >= 1".into()));
    }
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    if !path.exists() {
        return Err(VaultError::Io(format!(
            "daily note does not exist: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    expect_line_text(&content, line, expect_text)?;
    let todos = parse_todos(&content);
    let target = todos
        .iter()
        .find(|t| t.line == line)
        .ok_or_else(|| VaultError::Io(format!("line {line} is not a todo")))?;
    let mut drop: std::collections::HashSet<usize> = std::collections::HashSet::new();
    drop.insert(line);
    if with_children {
        let parent_depth = target.depth;
        for t in todos.iter().skip_while(|t| t.line <= line) {
            if t.depth <= parent_depth {
                break;
            }
            drop.insert(t.line);
        }
    }
    let next = without_lines(&content, &drop);
    write_atomic_with_undo(vault, date, &path, &content, &next)?;
    read_snapshot(vault, date)
}

/// Move a todo (and nested children) to the next calendar day.
///
/// Creates that note from the daily-note template when missing, without
/// rolling over the rest of the source day's open todos. A single `undo`
/// restores both notes (a tomorrow note created by the defer is deleted
/// again); if the source write fails after the destination append, the
/// destination is rolled back so the item is never duplicated.
///
/// When `heading` names a markdown section in the destination note, the item
/// is inserted there; otherwise the default placement applies.
pub fn defer_todo(
    vault: &Vault,
    date: NaiveDate,
    line: usize,
    expect_text: Option<&str>,
    with_children: bool,
    heading: Option<&str>,
) -> Result<Snapshot, VaultError> {
    defer_todo_to(vault, date, line, expect_text, with_children, heading, None)
}

/// Same as [`defer_todo`] but records undo to an explicit file when given.
/// Tests pass a per-test path so parallel threads never share the global
/// undo file.
pub fn defer_todo_to(
    vault: &Vault,
    date: NaiveDate,
    line: usize,
    expect_text: Option<&str>,
    with_children: bool,
    heading: Option<&str>,
    undo_dest: Option<&Path>,
) -> Result<Snapshot, VaultError> {
    if line == 0 {
        return Err(VaultError::Io("line must be >= 1".into()));
    }
    let next_date = date
        .checked_add_days(chrono::Days::new(1))
        .ok_or_else(|| VaultError::Io("date overflow".into()))?;
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    if !path.exists() {
        return Err(VaultError::Io(format!(
            "daily note does not exist: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    expect_line_text(&content, line, expect_text)?;
    let todos = parse_todos(&content);
    let target = todos
        .iter()
        .find(|t| t.line == line)
        .ok_or_else(|| VaultError::Io(format!("line {line} is not a todo")))?;
    let mut drop: std::collections::HashSet<usize> = std::collections::HashSet::new();
    drop.insert(line);
    if with_children {
        let parent_depth = target.depth;
        for t in todos.iter().skip_while(|t| t.line <= line) {
            if t.depth <= parent_depth {
                break;
            }
            drop.insert(t.line);
        }
    }
    let mut moving: Vec<(usize, bool, String)> = todos
        .into_iter()
        .filter(|t| drop.contains(&t.line))
        .map(|t| (t.depth, t.checked, t.text))
        .collect();
    let base_depth = moving[0].0;
    for item in &mut moving {
        item.0 = item.0.saturating_sub(base_depth);
    }

    let dest_path = resolved_note_path(vault, &config, next_date)?;
    if dest_path == path {
        return Err(VaultError::Io("cannot defer onto the same note".into()));
    }
    let dest_existed = dest_path.exists();
    let dest_before: Option<String> =
        if dest_existed {
            Some(fs::read_to_string(&dest_path).map_err(|e| {
                VaultError::Io(format!("failed to read {}: {e}", dest_path.display()))
            })?)
        } else {
            None
        };

    // Record both `before` states before mutating anything, so one undo
    // restores both notes. The destination write below is plain atomic
    // (no second undo record to clobber this one) and the source write is
    // too — both are covered here.
    let record = |files: &[(&Path, Option<&str>)]| match undo_dest {
        Some(dest) => crate::undo::record_before_multi_to(vault, date, files, dest),
        None => crate::undo::record_before_multi(vault, date, files),
    };
    let discard_record = || match undo_dest {
        Some(dest) => crate::undo::discard_at(dest),
        None => crate::undo::discard(),
    };
    record(&[
        (&path, Some(content.as_str())),
        (&dest_path, dest_before.as_deref()),
    ])?;

    // Create the destination from the template when missing (no carry-over:
    // deferring one todo must not pull the rest of today's list along).
    if let Err(e) = create_note_if_missing(vault, &config, &dest_path, next_date) {
        discard_record();
        return Err(e);
    }
    let dest_content = fs::read_to_string(&dest_path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", dest_path.display())))?;
    let dest_next = insert_todo_lines(
        &dest_content,
        &format_checkbox_lines(&moving),
        infer_todo_style(vault, &config, next_date, &dest_content),
        heading,
    );
    if let Err(e) = write_atomic(vault.root(), &dest_path, &dest_next) {
        if !dest_existed {
            let _ = fs::remove_file(&dest_path);
        }
        discard_record();
        return Err(e);
    }

    let next = without_lines(&content, &drop);
    if let Err(e) = write_atomic(vault.root(), &path, &next) {
        // Roll the destination back so a failed defer never duplicates.
        match dest_before {
            Some(before) => {
                let _ = write_atomic(vault.root(), &dest_path, &before);
            }
            None => {
                let _ = fs::remove_file(&dest_path);
            }
        }
        discard_record();
        return Err(e);
    }
    read_snapshot(vault, date)
}

pub fn set_indent(
    vault: &Vault,
    date: NaiveDate,
    line: usize,
    expect_text: Option<&str>,
    delta: i32,
) -> Result<Snapshot, VaultError> {
    if line == 0 {
        return Err(VaultError::Io("line must be >= 1".into()));
    }
    if delta == 0 {
        return read_snapshot(vault, date);
    }
    let config = vault.daily_notes_config()?;
    let path = resolved_note_path(vault, &config, date)?;
    if !path.exists() {
        return Err(VaultError::Io(format!(
            "daily note does not exist: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| VaultError::Io(format!("failed to read {}: {e}", path.display())))?;
    expect_line_text(&content, line, expect_text)?;
    let re = checkbox_re();
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let idx = line - 1;
    if idx >= lines.len() {
        return Err(VaultError::Io(format!("line {line} is past end of file")));
    }
    reject_fenced_line(&content, line)?;
    let caps = re
        .captures(&lines[idx])
        .ok_or_else(|| VaultError::Io(format!("line {line} is not a checkbox todo")))?;
    let mut level = indent_level(&caps[1]) as i32;
    level = (level + delta).max(0);
    let indent = "  ".repeat(level as usize);
    let new_line = format!(
        "{}{} [{}]{}{}",
        indent, &caps[2], &caps[3], &caps[4], &caps[5]
    );
    let next = rewrite_line(&content, line, &new_line);
    write_atomic_with_undo(vault, date, &path, &content, &next)?;
    read_snapshot(vault, date)
}

pub fn week_summary(
    vault: &Vault,
    anchor: NaiveDate,
) -> Result<crate::status::WeekSummary, VaultError> {
    week_summary_filtered(vault, anchor, None)
}

/// Like [`week_summary`], but when `heading` is set each day counts only the
/// todos under a matching markdown heading — the same filter the day view
/// uses, so the week strip cannot report work the panel says does not exist.
pub fn week_summary_filtered(
    vault: &Vault,
    anchor: NaiveDate,
    heading: Option<&str>,
) -> Result<crate::status::WeekSummary, VaultError> {
    use crate::status::{DaySummary, WeekSummary};
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    let days_from_mon = anchor.weekday().num_days_from_monday();
    let monday = anchor
        .checked_sub_days(chrono::Days::new(days_from_mon as u64))
        .unwrap_or(anchor);
    let mut days = Vec::with_capacity(7);
    for offset in 0..7u64 {
        let d = monday
            .checked_add_days(chrono::Days::new(offset))
            .unwrap_or(monday);
        let snap = read_snapshot_filtered(vault, d, heading)?;
        days.push(DaySummary {
            date: d.format("%Y-%m-%d").to_string(),
            open_count: snap.open_count.unwrap_or(0),
            done_count: snap.done_count.unwrap_or(0),
            exists: snap.exists.unwrap_or(false),
            is_today: d == today,
        });
    }
    Ok(WeekSummary {
        state: crate::status::State::Ok,
        days: Some(days),
        error_code: None,
        error: None,
    })
}

fn write_atomic_with_undo(
    vault: &Vault,
    date: NaiveDate,
    path: &Path,
    before: &str,
    after: &str,
) -> Result<(), VaultError> {
    crate::undo::record_before(vault, date, path, before)?;
    write_atomic(vault.root(), path, after)
}

/// Public wrapper used by the undo module to restore content.
pub fn write_atomic_public(
    vault_root: &Path,
    path: &Path,
    content: &str,
) -> Result<(), VaultError> {
    write_atomic(vault_root, path, content)
}

fn infer_todo_style(
    vault: &Vault,
    config: &DailyNotesConfig,
    date: NaiveDate,
    current: &str,
) -> TodoStyle {
    let mut style = DEFAULT_TODO_STYLE;
    let mut compact_known = false;
    let mut heading_gap_known = false;
    if apply_style_from_content(
        current,
        &mut style,
        &mut compact_known,
        &mut heading_gap_known,
    ) && compact_known
        && heading_gap_known
    {
        return style;
    }
    for offset in 1..=TODO_STYLE_LOOKBACK_DAYS {
        let Some(prev) = date.checked_sub_days(chrono::Days::new(offset)) else {
            break;
        };
        let Some(body) = note_body(vault, config, prev) else {
            continue;
        };
        if apply_style_from_content(
            &body,
            &mut style,
            &mut compact_known,
            &mut heading_gap_known,
        ) && compact_known
            && heading_gap_known
        {
            break;
        }
    }
    style
}

fn note_body(vault: &Vault, config: &DailyNotesConfig, date: NaiveDate) -> Option<String> {
    let path = resolved_note_path(vault, config, date).ok()?;
    if !path.exists() {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn apply_style_from_content(
    content: &str,
    style: &mut TodoStyle,
    compact_known: &mut bool,
    heading_gap_known: &mut bool,
) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    let fenced = fence_mask(&lines);
    let Some((start, end)) = first_todo_region(&lines, &fenced, 0, lines.len()) else {
        return false;
    };
    if !*heading_gap_known {
        style.blank_after_heading = start > 0 && lines[start - 1].trim().is_empty();
        *heading_gap_known = true;
    }
    if !*compact_known && let Some(compact) = compact_between(&lines, &fenced, start, end) {
        style.compact = compact;
        *compact_known = true;
    }
    *compact_known && *heading_gap_known
}

fn is_title_heading(line: &str) -> bool {
    // Any heading level counts as the date/title heading when it is the
    // note's first content line (e.g. `## 2026-08-20` in daily-notes
    // templates). The caller only checks the first line after frontmatter
    // and blank lines, so H1-only matching would insert above `##` titles.
    let t = line.trim();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && t[hashes..].starts_with([' ', '\t'])
}

fn skip_frontmatter(lines: &[&str]) -> usize {
    if lines.first().is_none_or(|l| l.trim() != "---") {
        return 0;
    }
    for (idx, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            return idx + 1;
        }
    }
    0
}

/// First checkbox run in `[from, to)`, including single blank lines between
/// items. Stops at headings, paragraphs, or other non-todo content.
fn first_todo_region(
    lines: &[&str],
    fenced: &[bool],
    from: usize,
    to: usize,
) -> Option<(usize, usize)> {
    let re = checkbox_re();
    let start = (from..to).find(|&i| !fenced[i] && re.is_match(lines[i]))?;
    let mut last = start;
    let mut i = start + 1;
    while i < to {
        if fenced[i] {
            break;
        }
        if re.is_match(lines[i]) {
            last = i;
            i += 1;
            continue;
        }
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        break;
    }
    Some((start, last + 1))
}

/// `true` when at least one adjacent todo pair has no blank line between them.
/// `None` when the range has fewer than two checkboxes.
fn compact_between(lines: &[&str], fenced: &[bool], start: usize, end: usize) -> Option<bool> {
    let re = checkbox_re();
    let mut prev: Option<usize> = None;
    let mut dense = false;
    let mut spaced = false;
    let mut count = 0usize;
    for i in start..end {
        if fenced[i] || !re.is_match(lines[i]) {
            continue;
        }
        count += 1;
        if let Some(p) = prev {
            let gap = (p + 1..i).any(|j| lines[j].trim().is_empty());
            if gap {
                spaced = true;
            } else {
                dense = true;
            }
        }
        prev = Some(i);
    }
    if count < 2 {
        return None;
    }
    Some(dense || !spaced)
}

fn insert_items(out: &mut Vec<String>, at: usize, items: &[String], compact: bool) -> usize {
    let mut pos = at;
    if !compact && pos > 0 && !out[pos - 1].trim().is_empty() {
        out.insert(pos, String::new());
        pos += 1;
    }
    for (offset, item) in items.iter().enumerate() {
        if !compact && offset > 0 {
            out.insert(pos, String::new());
            pos += 1;
        }
        out.insert(pos, item.clone());
        pos += 1;
    }
    pos
}

fn ensure_blank_before_following(out: &mut Vec<String>, after: usize) {
    if after < out.len() && !out[after].trim().is_empty() {
        out.insert(after, String::new());
    }
}

fn insert_todo_lines(
    content: &str,
    items: &[String],
    style: TodoStyle,
    heading: Option<&str>,
) -> String {
    let default_heading = tasks_heading_re();
    let any_heading = any_heading_re();
    let lines: Vec<&str> = content.lines().collect();
    let fenced = fence_mask(&lines);
    let mut out: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();

    // A configured heading wins when it names an existing section. A blank
    // or missing heading falls through to the default placement below, so
    // existing notes keep their behavior.
    if let Some(want) = heading.map(str::trim).filter(|s| !s.is_empty())
        && let Some((sec_start, sec_end)) = custom_section_bounds(&lines, want, any_heading)
    {
        let compact = first_todo_region(&lines, &fenced, sec_start, sec_end)
            .and_then(|(start, end)| compact_between(&lines, &fenced, start, end))
            .unwrap_or(style.compact);
        let (at, needs_boundary_blank) = tasks_section_insert_at(&lines, sec_start, sec_end);
        let after = insert_items(&mut out, at, items, compact);
        if needs_boundary_blank {
            ensure_blank_before_following(&mut out, after);
        }
        return finish_body(out);
    }

    if let Some((sec_start, sec_end)) = tasks_section_bounds(&lines, default_heading, any_heading) {
        let compact = first_todo_region(&lines, &fenced, sec_start, sec_end)
            .and_then(|(start, end)| compact_between(&lines, &fenced, start, end))
            .unwrap_or(style.compact);
        let (at, needs_boundary_blank) = tasks_section_insert_at(&lines, sec_start, sec_end);
        let after = insert_items(&mut out, at, items, compact);
        if needs_boundary_blank {
            ensure_blank_before_following(&mut out, after);
        }
        return finish_body(out);
    }

    if let Some((start, end)) = first_todo_region(&lines, &fenced, 0, lines.len()) {
        let compact = compact_between(&lines, &fenced, start, end).unwrap_or(style.compact);
        let after = insert_items(&mut out, end, items, compact);
        ensure_blank_before_following(&mut out, after);
        return finish_body(out);
    }

    let mut at = skip_frontmatter(&lines);
    while at < lines.len() && lines[at].trim().is_empty() {
        at += 1;
    }
    if at < lines.len() && is_title_heading(lines[at]) {
        at += 1;
    }
    let after_title = at;
    if style.blank_after_heading {
        if after_title < lines.len() && lines[after_title].trim().is_empty() {
            at = after_title + 1;
        } else if after_title > 0 {
            out.insert(after_title, String::new());
            at = after_title + 1;
        }
    } else {
        at = after_title;
    }
    let after = insert_items(&mut out, at, items, style.compact);
    ensure_blank_before_following(&mut out, after);
    finish_body(out)
}

fn tasks_section_bounds(
    lines: &[&str],
    heading: &Regex,
    any_heading: &Regex,
) -> Option<(usize, usize)> {
    let mut fences = FenceScanner::default();
    for (idx, line) in lines.iter().enumerate() {
        if fences.is_fenced(line) {
            continue;
        }
        if !heading.is_match(line) {
            continue;
        }
        return section_range_from(lines, idx, any_heading);
    }
    None
}

/// Bounds of the section under a custom heading title: exact trimmed-title
/// match, case-insensitive, any `#` level, first match wins — the same
/// semantics as the `todoHeading` display filter. A heading-like line inside
/// a fenced code block never starts or ends the section.
fn custom_section_bounds(
    lines: &[&str],
    want: &str,
    any_heading: &Regex,
) -> Option<(usize, usize)> {
    let want_l = want.trim().to_lowercase();
    if want_l.is_empty() {
        return None;
    }
    let mut fences = FenceScanner::default();
    for (idx, line) in lines.iter().enumerate() {
        if fences.is_fenced(line) {
            continue;
        }
        let Some(caps) = any_heading.captures(line) else {
            continue;
        };
        if caps[2].trim().to_lowercase() == want_l {
            return section_range_from(lines, idx, any_heading);
        }
    }
    None
}

/// Section content range starting after the heading at `idx`: until the next
/// same-or-higher heading, ignoring heading-like lines inside fenced code
/// blocks.
fn section_range_from(lines: &[&str], idx: usize, any_heading: &Regex) -> Option<(usize, usize)> {
    let level = any_heading.captures(lines[idx])?[1].len();
    let section_start = idx + 1;
    let mut section_end = section_start;
    let mut fences = FenceScanner::default();
    while section_end < lines.len() {
        if !fences.is_fenced(lines[section_end])
            && let Some(next) = any_heading.captures(lines[section_end])
            && next[1].len() <= level
        {
            break;
        }
        section_end += 1;
    }
    Some((section_start, section_end))
}

fn tasks_section_insert_at(
    lines: &[&str],
    section_start: usize,
    section_end: usize,
) -> (usize, bool) {
    let mut at = section_end;
    while at > section_start && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    let mut needs_boundary_blank = false;
    if at == section_start && section_start < section_end && lines[section_start].trim().is_empty()
    {
        at += 1;
        if at == section_end {
            needs_boundary_blank = true;
        }
    }
    (at, needs_boundary_blank)
}

/// Re-join a body's lines, preserving the file's trailing-newline byte: a body
/// that ended with a blank line keeps it. Every mutation used to inline this
/// with one of two slightly different guards, so `toggle`/`edit`/`indent`/
/// `add --under-line` silently dropped that final blank line while
/// `delete`/`defer` kept it.
fn join_body(content: &str, lines: &[String]) -> String {
    let mut next = lines.join("\n");
    if content.ends_with('\n') && !next.is_empty() {
        next.push('\n');
    }
    next
}

/// Rewrite one 1-based line, leaving every other byte as it was.
fn rewrite_line(content: &str, line: usize, new_line: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    lines[line - 1] = new_line.to_string();
    join_body(content, &lines)
}

/// Drop the given 1-based lines, leaving every other byte as it was.
fn without_lines(content: &str, drop: &std::collections::HashSet<usize>) -> String {
    let lines: Vec<String> = content
        .lines()
        .enumerate()
        .filter(|(i, _)| !drop.contains(&(i + 1)))
        .map(|(_, line)| line.to_string())
        .collect();
    join_body(content, &lines)
}

fn finish_body(out: Vec<String>) -> String {
    let mut body = out.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body
}

/// Reject a 1-based line that is a fence marker or sits inside a fenced code
/// block. The read path already ignores those lines, so a checkbox there is a
/// code sample the user does not consider a todo — a line-addressed mutation
/// (`toggle`/`edit`/`indent` without `--expect-text`) must not rewrite it.
fn reject_fenced_line(content: &str, line: usize) -> Result<(), VaultError> {
    let lines: Vec<&str> = content.lines().collect();
    if fence_mask(&lines).get(line - 1).copied().unwrap_or(false) {
        return Err(VaultError::Io(format!(
            "line {line} is inside a fenced code block"
        )));
    }
    Ok(())
}

fn toggle_line(content: &str, line: usize) -> Result<String, VaultError> {
    let re = checkbox_re();
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let idx = line - 1;
    if idx >= lines.len() {
        return Err(VaultError::Io(format!(
            "line {line} is past end of file ({} lines)",
            lines.len()
        )));
    }
    reject_fenced_line(content, line)?;
    let current = lines[idx].clone();
    let caps = re
        .captures(&current)
        .ok_or_else(|| VaultError::Io(format!("line {line} is not a checkbox todo")))?;
    let checked = !caps[3].eq(" ");
    let mark = if checked { " " } else { "x" };
    let new_line = format!(
        "{}{} [{}]{}{}",
        &caps[1], &caps[2], mark, &caps[4], &caps[5]
    );
    Ok(rewrite_line(content, line, &new_line))
}

/// Create `dir` and any missing parents after verifying its existing
/// ancestors stay inside the vault.
///
/// The nearest existing ancestor is canonicalized first, so a symlinked
/// daily-notes folder with a nested date format cannot cause directory
/// creation outside the vault: only plain, not-yet-existing directories are
/// created on top of the verified location.
fn create_dir_all_in_vault(vault_root: &Path, dir: &Path) -> Result<(), VaultError> {
    let root = vault_root.canonicalize().map_err(|e| {
        VaultError::Io(format!(
            "failed to resolve vault root {}: {e}",
            vault_root.display()
        ))
    })?;
    let anchor = dir
        .ancestors()
        .find(|a| a.exists())
        .ok_or_else(|| VaultError::Io(format!("no existing parent of {}", dir.display())))?;
    let real_anchor = anchor
        .canonicalize()
        .map_err(|e| VaultError::Io(format!("failed to resolve {}: {e}", anchor.display())))?;
    if !real_anchor.starts_with(&root) {
        return Err(VaultError::Io(format!(
            "refusing to create directories outside the vault: {} resolves to {}",
            anchor.display(),
            real_anchor.display()
        )));
    }
    fs::create_dir_all(dir)
        .map_err(|e| VaultError::Io(format!("failed to create {}: {e}", dir.display())))
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

/// Mode for the sibling temp file used by [`write_atomic`].
///
/// Must match an existing note (so a 0600 vault file is never briefly
/// recreated world-readable under umask 022) and default to owner-only for
/// new notes. Applied at `open` time — before any content is written.
#[cfg(unix)]
fn atomic_temp_mode(target: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(target)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(0o600)
}

fn write_atomic(vault_root: &Path, path: &Path, content: &str) -> Result<(), VaultError> {
    let target = resolve_note_target(vault_root, path)?;
    let tmp = temp_path_for(&target);
    let write_result = (|| {
        // `create_new` is O_CREAT|O_EXCL: it fails if anything — including a
        // symlink — already sits at the temp path, so a pre-created link
        // cannot redirect this write. The parent was canonicalized above, so
        // the temp file cannot escape through a symlinked directory either.
        //
        // On Unix, set the create mode before writing so vault content is
        // never briefly readable under a looser umask default (#7777).
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(atomic_temp_mode(&target))
                .open(&tmp)
                .map_err(|e| VaultError::Io(format!("failed to create {}: {e}", tmp.display())))?
        };
        #[cfg(not(unix))]
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| VaultError::Io(format!("failed to create {}: {e}", tmp.display())))?;
        file.write_all(content.as_bytes())
            .map_err(|e| VaultError::Io(format!("failed to write {}: {e}", tmp.display())))?;
        file.sync_all()
            .map_err(|e| VaultError::Io(format!("failed to sync {}: {e}", tmp.display())))?;
        Ok(())
    })();
    if let Err(err) = write_result {
        // Never leave an orphaned temp file behind in the vault.
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    // Re-apply the existing note's permissions after create (umask can clear
    // bits from OpenOptions::mode). New notes keep the owner-only create mode.
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(&target) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
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
        (
            Vault {
                root,
                archive: None,
            },
            date,
            note,
        )
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
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Todos\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn adds_under_tasks_heading() {
        let (vault, date, note) = vault_with("# Day\n\n## Tasks\n\n- [ ] existing\n\n## Notes\n");
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Tasks\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn appends_after_nested_content_in_todo_section() {
        let (vault, date, note) = vault_with(
            "# Day\n\n## Todos\n\n- [ ] first\n\n### Later\n\n- [ ] second\n\n## Notes\n",
        );
        add_todo(&vault, date, "last", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Todos\n\n- [ ] first\n\n### Later\n\n- [ ] second\n- [ ] last\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn ignores_heading_like_lines_inside_fenced_code_block() {
        let (vault, date, note) =
            vault_with("# Day\n\n## Todos\n\n- [ ] first\n\n```\n## fake\n```\n\n## Notes\n");
        add_todo(&vault, date, "last", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Todos\n\n- [ ] first\n\n```\n## fake\n```\n- [ ] last\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn fenced_section_ignores_shorter_and_mismatched_closers() {
        let (vault, date, note) = vault_with(
            "# Day\n\n## Todos\n\n- [ ] first\n\n````md\n```\n## fake\n~~~~\n```` still fenced\n## also fake\n`````   \n- [ ] second\n\n## Notes\n",
        );
        add_todo(&vault, date, "last", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Todos\n\n- [ ] first\n\n````md\n```\n## fake\n~~~~\n```` still fenced\n## also fake\n`````   \n- [ ] second\n- [ ] last\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn ignores_tasks_heading_inside_fenced_code_block() {
        let (vault, date, note) = vault_with(
            "# Day\n\n````md\n## Tasks\n```\n~~~~\n````\n\n## Tasks\n\n- [ ] existing\n\n## Notes\n",
        );
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n````md\n## Tasks\n```\n~~~~\n````\n\n## Tasks\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn preserves_blank_line_boundary_in_empty_section() {
        let (vault, date, note) = vault_with("# Day\n\n## Todos\n\n## Notes\n");
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(body, "# Day\n\n## Todos\n\n- [ ] new one\n\n## Notes\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn adds_under_custom_heading() {
        // The reported #32 template: todos live under `## Inbox` near the
        // top, with later sections below. The new item must land in Inbox,
        // not after `## Notes`.
        let (vault, date, note) = vault_with(
            "# Day\n\n## Focus\n\ntext\n\n## Inbox\n\n- [ ] existing\n\n## Notes\n\nbody\n",
        );
        add_todo(&vault, date, "new one", Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Focus\n\ntext\n\n## Inbox\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n\nbody\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn custom_heading_wins_over_tasks_section() {
        let (vault, date, note) = vault_with(
            "# Day\n\n## Inbox\n\n- [ ] in-inbox\n\n## Tasks\n\n- [ ] in-tasks\n\n## Notes\n",
        );
        add_todo(&vault, date, "new one", Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Inbox\n\n- [ ] in-inbox\n- [ ] new one\n\n## Tasks\n\n- [ ] in-tasks\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn custom_heading_matches_case_insensitively() {
        let (vault, date, note) = vault_with("# Day\n\n## inbox\n\n- [ ] existing\n\n## Notes\n");
        add_todo(&vault, date, "new one", Some("  Inbox  ")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## inbox\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn custom_heading_matches_indented_atx_heading_with_closing_hashes() {
        let (vault, date, note) =
            vault_with("# Day\n\n   ## Inbox ##\n\n- [ ] existing\n\n  ## Notes ##\n\nbody\n");
        add_todo(&vault, date, "new one", Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n   ## Inbox ##\n\n- [ ] existing\n- [ ] new one\n\n  ## Notes ##\n\nbody\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn custom_heading_uses_first_match() {
        let (vault, date, note) = vault_with(
            "# Day\n\n## Inbox\n\n- [ ] first\n\n## Notes\n\n## Inbox\n\n- [ ] second\n",
        );
        add_todo(&vault, date, "new one", Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Inbox\n\n- [ ] first\n- [ ] new one\n\n## Notes\n\n## Inbox\n\n- [ ] second\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn missing_custom_heading_falls_back_to_tasks() {
        let (vault, date, note) = vault_with("# Day\n\n## Tasks\n\n- [ ] existing\n\n## Notes\n");
        add_todo(&vault, date, "new one", Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Tasks\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn missing_custom_heading_falls_back_to_first_list() {
        let (vault, date, note) = vault_with("# Day\n\n- [ ] existing\n- [ ] also\n\n## Notes\n");
        add_todo(&vault, date, "new one", Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n- [ ] existing\n- [ ] also\n- [ ] new one\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn custom_heading_ignores_fenced_code_block() {
        // A `## Inbox` line inside a fence must not become the insert
        // target, and a peer-looking heading inside the custom section's
        // fence must not end the section early.
        let (vault, date, note) = vault_with(
            "# Day\n\n```\n## Inbox\n```\n\n## Inbox\n\n- [ ] first\n\n```\n## Notes\n```\n- [ ] second\n\n## Notes\n",
        );
        add_todo(&vault, date, "last", Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n```\n## Inbox\n```\n\n## Inbox\n\n- [ ] first\n\n```\n## Notes\n```\n- [ ] second\n- [ ] last\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn blank_custom_heading_uses_default_placement() {
        let (vault, date, note) = vault_with("# Day\n\n## Todos\n\n- [ ] existing\n\n## Notes\n");
        add_todo(&vault, date, "new one", Some("   ")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Todos\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn add_under_line_ignores_custom_heading() {
        // An explicit parent wins over the configured section.
        let (vault, date, note) =
            vault_with("# Day\n\n## Inbox\n\n- [ ] inbox-item\n\n## Notes\n\n- [ ] parent\n");
        add_todo_under(&vault, date, "child", Some(9), Some("Inbox")).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Inbox\n\n- [ ] inbox-item\n\n## Notes\n\n- [ ] parent\n  - [ ] child\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn line_addressed_mutations_reject_a_fenced_line() {
        // The read path ignores checkboxes inside a fence; a line-addressed
        // mutation without `--expect-text` must not rewrite one either.
        let body = "# Day\n\n```\n- [ ] sample\n```\n\n- [ ] real\n";
        let (vault, today, note) = vault_with(body);
        for (label, result) in [
            ("toggle", toggle_todo(&vault, today, 4, None).map(|_| ())),
            (
                "edit",
                edit_todo(&vault, today, 4, None, "changed").map(|_| ()),
            ),
            ("indent", set_indent(&vault, today, 4, None, 1).map(|_| ())),
        ] {
            let err = result.expect_err(&format!("{label} must reject a fenced line"));
            assert!(
                err.to_string().contains("fenced"),
                "{label}: unclear error {err}"
            );
            assert_eq!(
                fs::read_to_string(&note).unwrap(),
                body,
                "{label} rewrote the note"
            );
        }
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_keeps_the_parent_when_identical_text_lines_nest() {
        // Two candidates share the text and the nesting. The target can account
        // for one, so one line goes — and it must be the child, not the parent,
        // or the survivor is left orphaned at the top level.
        let (vault, today, _) = vault_with("- [ ] water\n");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&ynote, "- [ ] water\n  - [ ] water\n").unwrap();
        carry_over(&vault, today, None).unwrap();
        let ybody = fs::read_to_string(&ynote).unwrap();
        assert_eq!(
            ybody, "- [ ] water\n",
            "the parent must survive with its child removed: {ybody:?}"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn mutations_keep_the_trailing_blank_line() {
        // The two variants of the trailing-newline guard meant some commands
        // silently dropped a note's final blank line. Every mutation must leave
        // the tail byte-identical.
        let body = "# Day\n\n- [ ] a\n\n";
        let check = |label: &str, after: String| {
            assert!(
                after.ends_with("\n\n"),
                "{label} dropped the trailing blank line: {after:?}"
            );
        };

        let (vault, today, note) = vault_with(body);
        toggle_todo(&vault, today, 3, None).unwrap();
        check("toggle", fs::read_to_string(&note).unwrap());
        let _ = fs::remove_dir_all(vault.root());

        let (vault, today, note) = vault_with(body);
        edit_todo(&vault, today, 3, None, "b").unwrap();
        check("edit", fs::read_to_string(&note).unwrap());
        let _ = fs::remove_dir_all(vault.root());

        let (vault, today, note) = vault_with(body);
        set_indent(&vault, today, 3, None, 1).unwrap();
        check("indent", fs::read_to_string(&note).unwrap());
        let _ = fs::remove_dir_all(vault.root());

        let (vault, today, note) = vault_with(body);
        add_todo_under(&vault, today, "child", Some(3), None).unwrap();
        check("add --under-line", fs::read_to_string(&note).unwrap());
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn fenced_checkboxes_are_not_listed_or_counted() {
        let (vault, today, _) =
            vault_with("# Day\n\n```md\n- [ ] sample\n- [x] done-sample\n```\n\n- [ ] real\n");
        let snap = read_snapshot(&vault, today).unwrap();
        assert_eq!(
            snap.open_count,
            Some(1),
            "a fenced open checkbox was counted"
        );
        assert_eq!(
            snap.done_count,
            Some(0),
            "a fenced done checkbox was counted"
        );
        let texts: Vec<String> = snap.todos.unwrap().into_iter().map(|t| t.text).collect();
        assert_eq!(texts, vec!["real".to_string()]);
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn heading_filter_skips_fenced_lines() {
        // A fence inside the section must not become part of the todo run, and
        // a heading-looking line inside it must not end the section.
        let content = "# Day\n\n## Tasks\n\n```\n# Not a heading\n- [ ] fenced\n```\n\n- [ ] real\n\n# Later\n\n- [ ] outside\n";
        let filtered = filter_todos_by_heading(content, parse_todos(content), Some("Tasks"));
        let texts: Vec<&str> = filtered.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["real"], "got {texts:?}");
    }

    #[test]
    fn add_without_a_heading_inserts_outside_a_fence() {
        let (vault, today, note) = vault_with("# Day\n\n```\n- [ ] sample\n```\n");
        add_todo_under(&vault, today, "new", None, None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        let new_pos = body.find("- [ ] new").expect("todo was not written");
        let fence_pos = body.find("```").expect("fence disappeared");
        assert!(
            new_pos < fence_pos,
            "the new todo was written inside/after the fence: {body:?}"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_keeps_duplicate_source_lines_beyond_the_target_count() {
        // Two identical open todos in the source, one same-text todo already in
        // the target. The target can account for one, so exactly one source
        // line may go — the other is not a stranded move and must survive.
        let (vault, today, _) = vault_with("- [ ] water\n");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&ynote, "- [ ] water\n- [ ] water\n").unwrap();
        carry_over(&vault, today, None).unwrap();
        let ybody = fs::read_to_string(&ynote).unwrap();
        assert_eq!(
            ybody.matches("water").count(),
            1,
            "only one source line may be removed: {ybody:?}"
        );
        let target = fs::read_to_string(vault.root().join("Daily/2026-08-20.md")).unwrap();
        assert_eq!(
            target.matches("water").count(),
            1,
            "the target must not gain a duplicate: {target:?}"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn out_of_range_line_numbers_are_rejected_without_touching_the_note() {
        // `rewrite_line` indexes `line - 1` directly, so its safety rests on
        // every caller validating first. That contract needs a test: a future
        // call site that forgets it would turn a clean `Err` into a panic in an
        // always-on backend.
        let body = "# Day\n\n- [ ] a\n";
        let (vault, today, note) = vault_with(body);
        for line in [0usize, 99] {
            assert!(
                toggle_todo(&vault, today, line, None).is_err(),
                "toggle {line}"
            );
            assert!(
                edit_todo(&vault, today, line, None, "x").is_err(),
                "edit {line}"
            );
            assert!(
                set_indent(&vault, today, line, None, 1).is_err(),
                "indent {line}"
            );
            assert!(
                delete_todo(&vault, today, line, None, false).is_err(),
                "delete {line}"
            );
            assert!(
                defer_todo(&vault, today, line, None, false, None).is_err(),
                "defer {line}"
            );
            assert_eq!(
                fs::read_to_string(&note).unwrap(),
                body,
                "line {line} changed the note"
            );
        }
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn read_and_insert_agree_on_indented_and_closed_headings() {
        // `add --heading Tasks` inserts under `  ## Tasks` or `# Tasks #`; the
        // read filter must see the same section, or the todo that was just
        // added is invisible (and repeated adds pile up).
        for heading in ["  ## Tasks", "# Tasks #", "## Tasks"] {
            let (vault, today, _) = vault_with(&format!(
                "# Day\n\n{heading}\n\n- [ ] existing\n\n# Later\n\n- [ ] outside\n"
            ));
            add_todo_under(&vault, today, "added", None, Some("Tasks")).unwrap();
            let snap = read_snapshot_filtered(&vault, today, Some("Tasks")).unwrap();
            let todos = snap.todos.unwrap();
            let texts: Vec<&str> = todos.iter().map(|t| t.text.as_str()).collect();
            assert!(
                texts.contains(&"added"),
                "todo added under {heading:?} is not visible: {texts:?}"
            );
            assert!(
                texts.contains(&"existing"),
                "existing todo under {heading:?} is not visible: {texts:?}"
            );
            assert!(
                !texts.contains(&"outside"),
                "the section for {heading:?} leaked past the next heading: {texts:?}"
            );
            let _ = fs::remove_dir_all(vault.root());
        }
    }

    #[test]
    fn carry_over_respects_custom_heading() {
        let (vault, today, _) =
            vault_with("# Day\n\n## Inbox\n\n- [ ] today-only\n\n## Notes\n\nbody\n");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&ynote, "- [ ] leftover\n- [x] finished\n").unwrap();
        carry_over(&vault, today, Some("Inbox")).unwrap();
        let body = fs::read_to_string(vault.root().join("Daily/2026-08-20.md")).unwrap();
        assert_eq!(
            body,
            "# Day\n\n## Inbox\n\n- [ ] today-only\n- [ ] leftover\n\n## Notes\n\nbody\n"
        );
        assert_eq!(fs::read_to_string(&ynote).unwrap(), "- [x] finished\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn defer_respects_custom_heading() {
        let (vault, date, note) = vault_with("- [ ] keep\n- [ ] later\n");
        fs::write(
            vault.root().join("Daily/2026-08-21.md"),
            "# Next\n\n## Inbox\n\n- [ ] existing\n\n## Notes\n",
        )
        .unwrap();
        defer_todo(&vault, date, 2, Some("later"), false, Some("Inbox")).unwrap();
        assert_eq!(fs::read_to_string(&note).unwrap(), "- [ ] keep\n");
        let body = fs::read_to_string(vault.root().join("Daily/2026-08-21.md")).unwrap();
        assert_eq!(
            body,
            "# Next\n\n## Inbox\n\n- [ ] existing\n- [ ] later\n\n## Notes\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn appends_when_no_tasks_heading() {
        let (vault, date, note) = vault_with("# Day\n\nhello\n");
        add_todo(&vault, date, "solo", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(body, "# Day\n\n- [ ] solo\n\nhello\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn appends_to_first_todo_list_not_end_of_note() {
        let (vault, date, note) =
            vault_with("# Day\n\n- [ ] existing\n- [ ] also\n\n## Notes\n\nbody\n\n- [ ] stray\n");
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "# Day\n\n- [ ] existing\n- [ ] also\n- [ ] new one\n\n## Notes\n\nbody\n\n- [ ] stray\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn keeps_compact_todo_spacing() {
        let (vault, date, note) = vault_with("# Day\n\n- [ ] existing\n\n## Notes\n");
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(body, "# Day\n\n- [ ] existing\n- [ ] new one\n\n## Notes\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn infers_compact_spacing_from_recent_notes() {
        let (vault, date, note) = vault_with("# Day\n\n## Notes\n");
        let prev = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&prev, "# Prev\n\n- [ ] one\n- [ ] two\n\n## Notes\n").unwrap();
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(body, "# Day\n\n- [ ] new one\n\n## Notes\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn infers_spaced_todos_from_recent_notes() {
        let (vault, date, note) = vault_with("# Day\n");
        let prev = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&prev, "# Prev\n\n- [ ] one\n\n- [ ] two\n").unwrap();
        add_todo(&vault, date, "new one", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(body, "# Day\n\n- [ ] new one\n");
        add_todo(&vault, date, "second", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(body, "# Day\n\n- [ ] new one\n\n- [ ] second\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn inserts_after_frontmatter_and_title() {
        let (vault, date, note) =
            vault_with("---\ntags: daily\n---\n\n# Day\n\n## Notes\n\nbody\n");
        add_todo(&vault, date, "solo", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(
            body,
            "---\ntags: daily\n---\n\n# Day\n\n- [ ] solo\n\n## Notes\n\nbody\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn inserts_below_h2_date_heading() {
        // `## Day` as first content line is a date heading, not a section to
        // insert above (pullfrog review on #33).
        let (vault, date, note) = vault_with("## Day\n");
        add_todo(&vault, date, "solo", None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert_eq!(body, "## Day\n\n- [ ] solo\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn toggles_checkbox() {
        let (vault, date, note) = vault_with("- [ ] open\n- [x] done\n");
        toggle_todo(&vault, date, 1, None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.starts_with("- [x] open\n"));
        toggle_todo(&vault, date, 2, None).unwrap();
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.contains("- [ ] done\n"));
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn toggles_when_expected_text_matches() {
        let (vault, date, note) = vault_with("- [ ] open\n- [x] done\n");
        toggle_todo(&vault, date, 1, Some("open")).unwrap();
        assert!(
            fs::read_to_string(&note)
                .unwrap()
                .starts_with("- [x] open\n")
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn refuses_toggle_on_stale_line() {
        let (vault, date, note) = vault_with("- [ ] first\n- [ ] second\n");
        // The snapshot was rendered before a second todo was inserted above;
        // line 1 now holds different text than the caller saw.
        let err = toggle_todo(&vault, date, 1, Some("second")).unwrap_err();
        assert!(err.to_string().contains("stale view"), "err: {err}");
        assert_eq!(
            fs::read_to_string(&note).unwrap(),
            "- [ ] first\n- [ ] second\n"
        );
        // A line that is no longer a todo at all is also refused.
        let err = toggle_todo(&vault, date, 1, Some("gone")).unwrap_err();
        assert!(err.to_string().contains("stale view"), "err: {err}");
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
        let vault = Vault {
            root: root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        add_todo(&vault, date, "first", None).unwrap();
        let note = root.join("Daily/2026-08-20.md");
        let body = fs::read_to_string(&note).unwrap();
        assert!(body.contains("# 2026-08-20"));
        assert!(body.contains("## Tasks"));
        assert!(body.contains("- [ ] first"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepares_missing_note_from_template_before_open() {
        let root = unique_temp("open-create");
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
        let vault = Vault {
            root: root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();

        let snap = prepare_note_for_open(&vault, date).unwrap();

        assert_eq!(snap.exists, Some(true));
        assert_eq!(
            fs::read_to_string(root.join("Daily/2026-08-21.md")).unwrap(),
            "# 2026-08-21\n\n## Tasks\n\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preparing_future_note_does_not_carry_over_todos() {
        let root = unique_temp("open-future");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("Daily")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        fs::write(root.join("Daily/2026-08-20.md"), "- [ ] keep here\n").unwrap();
        let vault = Vault {
            root: root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();

        prepare_note_for_open(&vault, date).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("Daily/2026-08-20.md")).unwrap(),
            "- [ ] keep here\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("Daily/2026-08-21.md")).unwrap(),
            "# 2026-08-21\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preparing_archived_note_does_not_create_a_live_copy() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let (vault, archived) = archived_vault(date, "# Archived\n");

        let snap = prepare_note_for_open(&vault, date).unwrap();

        assert_eq!(snap.path, Some(archived.display().to_string()));
        assert_eq!(fs::read_to_string(&archived).unwrap(), "# Archived\n");
        assert!(!vault.root().join("dailies/2026-08-20.md").exists());
        let _ = fs::remove_dir_all(vault.root());
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
        let snap = carry_over(&vault, today, None).unwrap();
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
        let snap = carry_over(&vault, today, None).unwrap();
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
            archive: None,
        };
        assert!(!ensure_note(&vault, &config, &note, date, None).unwrap());
        assert_eq!(fs::read_to_string(&note).unwrap(), "keep\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_moves_open_from_previous_day() {
        let (vault, today, _) = vault_with("- [ ] today-only\n");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&ynote, "- [ ] leftover\n- [x] finished\n").unwrap();
        let snap = carry_over(&vault, today, None).unwrap();
        let texts: Vec<_> = snap.todos.unwrap().into_iter().map(|t| t.text).collect();
        assert!(texts.contains(&"leftover".to_string()));
        assert!(texts.contains(&"today-only".to_string()));
        assert!(!texts.iter().any(|t| t == "finished"));
        // Moved, not copied: the previous note keeps only its done todos.
        assert_eq!(fs::read_to_string(&ynote).unwrap(), "- [x] finished\n");
        // Idempotent: second carry-over does not duplicate.
        let snap2 = carry_over(&vault, today, None).unwrap();
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

    #[test]
    fn carry_over_completes_interrupted_move() {
        // Simulate a crash between "add to today" and "remove from
        // yesterday": the todo is open in both notes. The next carry-over
        // must still empty the previous note instead of stranding it.
        let (vault, today, _) = vault_with("- [ ] ghost\n");
        let ynote = vault.root().join("Daily").join("2026-08-19.md");
        fs::write(&ynote, "- [ ] ghost\n- [x] done\n").unwrap();
        carry_over(&vault, today, None).unwrap();
        assert_eq!(fs::read_to_string(&ynote).unwrap(), "- [x] done\n");
        let texts: Vec<_> = read_snapshot(&vault, today)
            .unwrap()
            .todos
            .unwrap()
            .into_iter()
            .map(|t| t.text)
            .collect();
        assert_eq!(texts.iter().filter(|t| *t == "ghost").count(), 1);
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_into_missing_note_does_not_duplicate() {
        // First mutation of the day: the target note does not exist yet.
        // Creating it must not re-enter the rollover and append every
        // carried item twice.
        let root = unique_temp("carry-missing");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("Daily")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        fs::write(
            root.join("Daily/2026-08-19.md"),
            "- [ ] drag along\n  - [ ] nested\n- [x] done yesterday\n",
        )
        .unwrap();
        let vault = Vault {
            root: root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let snap = carry_over(&vault, date, None).unwrap();
        let todos = snap.todos.unwrap();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].text, "drag along");
        assert_eq!(todos[0].depth, 0);
        assert_eq!(todos[1].text, "nested");
        assert_eq!(todos[1].depth, 1);
        let body = fs::read_to_string(vault.root().join("Daily/2026-08-20.md")).unwrap();
        assert_eq!(body.matches("- [ ] drag along").count(), 1, "body: {body}");
        assert_eq!(body.matches("- [ ] nested").count(), 1, "body: {body}");
        assert_eq!(
            fs::read_to_string(vault.root().join("Daily/2026-08-19.md")).unwrap(),
            "- [x] done yesterday\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn creating_note_rolls_previous_day_over() {
        let root = unique_temp("rollover");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("Daily")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        fs::write(
            root.join("Daily/2026-08-19.md"),
            "- [ ] drag along\n  - [ ] nested\n- [x] done yesterday\n",
        )
        .unwrap();
        let vault = Vault {
            root: root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        // First write of the new day creates the note and rolls yesterday's
        // open todos into it.
        add_todo(&vault, date, "fresh", None).unwrap();
        let body = fs::read_to_string(vault.root().join("Daily/2026-08-20.md")).unwrap();
        assert!(body.contains("- [ ] drag along\n  - [ ] nested\n"));
        assert!(body.contains("- [ ] fresh"));
        assert_eq!(
            fs::read_to_string(vault.root().join("Daily/2026-08-19.md")).unwrap(),
            "- [x] done yesterday\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn defer_moves_todo_to_next_day() {
        let (vault, date, note) = vault_with("- [ ] keep\n- [ ] later\n");
        defer_todo(&vault, date, 2, Some("later"), false, None).unwrap();
        assert_eq!(fs::read_to_string(&note).unwrap(), "- [ ] keep\n");
        let next = vault.root().join("Daily/2026-08-21.md");
        let body = fs::read_to_string(&next).unwrap();
        assert!(body.contains("- [ ] later"), "body: {body}");
        assert!(!body.contains("- [ ] keep"), "body: {body}");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn defer_moves_children_and_preserves_checked() {
        let (vault, date, note) =
            vault_with("- [ ] parent\n  - [x] child\n    - [ ] grand\n- [ ] sibling\n");
        defer_todo(&vault, date, 1, Some("parent"), true, None).unwrap();
        assert_eq!(fs::read_to_string(&note).unwrap(), "- [ ] sibling\n");
        let body = fs::read_to_string(vault.root().join("Daily/2026-08-21.md")).unwrap();
        assert!(
            body.contains("- [ ] parent\n  - [x] child\n    - [ ] grand\n"),
            "body: {body}"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn defer_creates_next_day_without_rolling_over_the_rest() {
        let (vault, date, note) = vault_with("- [ ] stay\n- [ ] move me\n");
        defer_todo(&vault, date, 2, Some("move me"), true, None).unwrap();
        assert_eq!(fs::read_to_string(&note).unwrap(), "- [ ] stay\n");
        let next = fs::read_to_string(vault.root().join("Daily/2026-08-21.md")).unwrap();
        assert!(next.contains("- [ ] move me"), "next: {next}");
        assert!(!next.contains("- [ ] stay"), "next: {next}");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn defer_rebases_nested_item_to_top_level() {
        let (vault, date, note) = vault_with("- [ ] parent\n  - [ ] child\n");
        defer_todo(&vault, date, 2, Some("child"), true, None).unwrap();
        assert_eq!(fs::read_to_string(&note).unwrap(), "- [ ] parent\n");
        let next = fs::read_to_string(vault.root().join("Daily/2026-08-21.md")).unwrap();
        assert!(next.contains("- [ ] child"), "next: {next}");
        assert!(!next.contains("  - [ ] child"), "next: {next}");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn defer_to_archived_tomorrow_uses_archive_in_place() {
        // Tomorrow exists only in the archive folder: defer must append there
        // instead of creating a live duplicate.
        let root = unique_temp("defer-archived");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("dailies")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"dailies","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        fs::write(root.join("dailies/2026-08-20.md"), "- [ ] move me\n").unwrap();
        let archived_tomorrow = root.join("dailies/_archive/2026/2026-08-21.md");
        fs::create_dir_all(archived_tomorrow.parent().unwrap()).unwrap();
        fs::write(&archived_tomorrow, "# Archived\n").unwrap();
        let vault = Vault {
            root: root.clone(),
            archive: Some("dailies/_archive/YYYY".into()),
        };
        defer_todo(&vault, date, 1, Some("move me"), false, None).unwrap();
        assert!(
            !root.join("dailies/2026-08-21.md").exists(),
            "defer must not create a live copy when tomorrow is archived"
        );
        let body = fs::read_to_string(&archived_tomorrow).unwrap();
        assert!(body.contains("- [ ] move me"), "archived: {body}");
        assert_eq!(
            fs::read_to_string(root.join("dailies/2026-08-20.md")).unwrap(),
            ""
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn add_todo_edits_archived_note_in_place() {
        // Guard for the add_checkbox_lines path fix: with only an archived
        // note present, adding must not spawn a live duplicate.
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let (vault, archived) = archived_vault(date, "# Archived\n");
        add_todo(&vault, date, "new one", None).unwrap();
        assert!(
            !vault.root().join("dailies/2026-08-20.md").exists(),
            "add must not create a live copy when the note is archived"
        );
        let body = fs::read_to_string(&archived).unwrap();
        assert!(body.contains("- [ ] new one"), "archived: {body}");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_preserves_note_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (vault, date, note) = vault_with("- [ ] open\n");
        fs::set_permissions(&note, fs::Permissions::from_mode(0o600)).unwrap();
        toggle_todo(&vault, date, 1, None).unwrap();
        let mode = fs::metadata(&note).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(vault.root());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_temp_mode_matches_existing_note_or_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (vault, _date, note) = vault_with("- [ ] open\n");
        fs::set_permissions(&note, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(atomic_temp_mode(&note), 0o600);
        fs::set_permissions(&note, fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(atomic_temp_mode(&note), 0o640);
        let missing = vault.root().join("Daily/does-not-exist-yet.md");
        assert_eq!(atomic_temp_mode(&missing), 0o600);
        let _ = fs::remove_dir_all(vault.root());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_creates_new_notes_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = unique_temp("new-mode");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        let vault = Vault {
            root: root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        add_todo(&vault, date, "brand new", None).unwrap();
        let note = root.join("Daily/2026-08-20.md");
        let mode = fs::metadata(&note).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(&root);
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
        let err = add_todo(&vault, date, "nope", None).unwrap_err();
        assert!(err.to_string().contains("outside the vault"), "err: {err}");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        let _ = fs::remove_dir_all(vault.root());
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_note_creation_through_symlinked_folder_with_nested_format() {
        use std::os::unix::fs::symlink;
        let root = unique_temp("nested-symlink");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY/MM-DD"}"#,
        )
        .unwrap();
        let outside = unique_temp("outside-nested");
        fs::create_dir_all(&outside).unwrap();
        let daily = root.join("Daily");
        symlink(&outside, &daily).unwrap();
        let vault = Vault {
            root: root.clone(),
            archive: None,
        };
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        // Note creation must fail before any directory is created through
        // the symlinked folder outside the vault.
        let err = add_todo(&vault, date, "nope", None).unwrap_err();
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
        let err = toggle_todo(&vault, date, 1, None).unwrap_err();
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
        toggle_todo(&vault, date, 1, None).unwrap();
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
        toggle_todo(&vault, date, 1, None).unwrap();
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

    /// Vault with an archive pattern configured and an archived note for
    /// `date` under `dailies/_archive/YYYY`, with no live note.
    fn archived_vault(date: NaiveDate, content: &str) -> (Vault, std::path::PathBuf) {
        let root = unique_temp("archived");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"dailies","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        let archived = root
            .join("dailies/_archive")
            .join(date.format("%Y").to_string())
            .join(date.format("%Y-%m-%d").to_string())
            .with_extension("md");
        fs::create_dir_all(archived.parent().unwrap()).unwrap();
        fs::write(&archived, content).unwrap();
        let vault = Vault {
            root,
            archive: Some("dailies/_archive/YYYY".into()),
        };
        (vault, archived)
    }

    #[test]
    fn read_snapshot_finds_archived_note() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let (vault, archived) =
            archived_vault(date, "# 2026-08-20\n- [ ] old one\n- [x] old done\n");
        let snap = read_snapshot(&vault, date).unwrap();
        assert_eq!(snap.exists, Some(true));
        assert_eq!(snap.path, Some(archived.display().to_string()));
        let todos = snap.todos.unwrap();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].text, "old one");
        assert!(!todos[0].checked);
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn week_summary_counts_archived_days() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 19).unwrap(); // Wednesday
        let (vault, _) = archived_vault(date, "- [ ] open from archive\n- [x] done from archive\n");
        let week = week_summary(&vault, date).unwrap();
        let days = week.days.unwrap();
        let wed = days
            .iter()
            .find(|d| d.date == "2026-08-19")
            .expect("wednesday in week");
        assert!(wed.exists);
        assert_eq!(wed.open_count, 1);
        assert_eq!(wed.done_count, 1);
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn toggle_todo_edits_archived_note_in_place() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let (vault, archived) = archived_vault(date, "- [ ] stale task\n");
        let snap = toggle_todo(&vault, date, 1, None).unwrap();
        assert_eq!(snap.exists, Some(true));
        assert_eq!(fs::read_to_string(&archived).unwrap(), "- [x] stale task\n");
        // The live path must not have been created.
        assert!(!vault.root().join("dailies/2026-08-20.md").exists());
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_reads_open_todos_from_archived_previous_day() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        let (vault, _) = archived_vault(
            date.checked_sub_days(chrono::Days::new(1)).unwrap(),
            "- [ ] leftover\n- [x] finished\n",
        );
        // Create today's live note so carry-over has a target.
        let today = vault.root().join("dailies/2026-08-20.md");
        fs::create_dir_all(today.parent().unwrap()).unwrap();
        fs::write(&today, "- [ ] already here\n").unwrap();
        carry_over(&vault, date, None).unwrap();
        let body = fs::read_to_string(&today).unwrap();
        assert!(body.contains("leftover"));
        assert!(body.contains("already here"));
        // Archived note keeps only done todos after the move.
        let prev = date.checked_sub_days(chrono::Days::new(1)).unwrap();
        let prev_path = vault
            .root()
            .join("dailies/_archive")
            .join(prev.format("%Y").to_string())
            .join(prev.format("%Y-%m-%d").to_string())
            .with_extension("md");
        assert_eq!(fs::read_to_string(&prev_path).unwrap(), "- [x] finished\n");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_skips_missing_yesterday_to_last_found() {
        // Yesterday missing, day before has open todos: the button count and
        // the move must use the last found note.
        let (vault, today, _) = vault_with("- [ ] today-only\n");
        fs::write(
            vault.root().join("Daily/2026-08-18.md"),
            "- [ ] from-two-days-ago\n- [x] done\n",
        )
        .unwrap();
        assert!(!vault.root().join("Daily/2026-08-19.md").exists());
        let snap = read_snapshot(&vault, today).unwrap();
        assert_eq!(snap.carry_over_count, Some(1));
        let snap = carry_over(&vault, today, None).unwrap();
        let texts: Vec<_> = snap.todos.unwrap().into_iter().map(|t| t.text).collect();
        assert!(texts.contains(&"from-two-days-ago".to_string()));
        assert_eq!(
            fs::read_to_string(vault.root().join("Daily/2026-08-18.md")).unwrap(),
            "- [x] done\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_skips_done_yesterday_to_last_found() {
        // Yesterday exists but holds only done todos: look further back.
        let (vault, today, _) = vault_with("- [ ] today-only\n");
        fs::write(
            vault.root().join("Daily/2026-08-19.md"),
            "- [x] finished yesterday\n",
        )
        .unwrap();
        fs::write(
            vault.root().join("Daily/2026-08-18.md"),
            "- [ ] older leftover\n",
        )
        .unwrap();
        let snap = read_snapshot(&vault, today).unwrap();
        assert_eq!(snap.carry_over_count, Some(1));
        carry_over(&vault, today, None).unwrap();
        assert!(
            fs::read_to_string(vault.root().join("Daily/2026-08-20.md"))
                .unwrap()
                .contains("older leftover")
        );
        // Yesterday untouched, older note keeps only done (none left).
        assert_eq!(
            fs::read_to_string(vault.root().join("Daily/2026-08-19.md")).unwrap(),
            "- [x] finished yesterday\n"
        );
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_prefers_nearest_day_with_open_todos() {
        // Both yesterday and the day before have open todos: carry the
        // nearest first; a second run drains the older backlog.
        let (vault, today, _) = vault_with("");
        fs::write(vault.root().join("Daily/2026-08-19.md"), "- [ ] near\n").unwrap();
        fs::write(vault.root().join("Daily/2026-08-18.md"), "- [ ] far\n").unwrap();
        let snap = read_snapshot(&vault, today).unwrap();
        assert_eq!(snap.carry_over_count, Some(1));
        carry_over(&vault, today, None).unwrap();
        let body = fs::read_to_string(vault.root().join("Daily/2026-08-20.md")).unwrap();
        assert!(body.contains("- [ ] near"), "body: {body}");
        assert!(!body.contains("- [ ] far"), "body: {body}");
        carry_over(&vault, today, None).unwrap();
        let body = fs::read_to_string(vault.root().join("Daily/2026-08-20.md")).unwrap();
        assert!(body.contains("- [ ] far"), "body: {body}");
        let _ = fs::remove_dir_all(vault.root());
    }

    #[test]
    fn carry_over_with_no_previous_open_is_noop() {
        let (vault, today, _) = vault_with("- [ ] today-only\n");
        let snap = read_snapshot(&vault, today).unwrap();
        assert_eq!(snap.carry_over_count, Some(0));
        let snap = carry_over(&vault, today, None).unwrap();
        assert_eq!(snap.carry_over_count, Some(0));
        let _ = fs::remove_dir_all(vault.root());
    }
}
