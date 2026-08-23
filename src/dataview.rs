//! Parse ```` ```dataview ```` TASK queries embedded in the daily note and
//! collect matching checkbox todos from the notes they reference.
//!
//! Only a conservative subset of the dataview query language is supported:
//! `TASK` with `FROM "path" OR "path"` (vault-relative folder or note paths)
//! and `WHERE completed` / `WHERE !completed` / `WHERE contains(text, "…")`
//! combined with AND. Any other clause marks the whole block unsupported and
//! it contributes no tasks — guessing dataview semantics would silently show
//! the wrong items. `dataviewjs` blocks are never interpreted.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{Vault, VaultError, ensure_under_root, push_safe_component, sanitize_rel};
use crate::status::TodoItem;
use crate::todos::parse_todos;

/// Cap on files scanned per FROM folder so a `FROM ""` (whole vault) query
/// cannot make the watch loop stall on huge vaults.
const MAX_FILES_PER_FOLDER: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskQuery {
    /// Vault-relative folder or note paths from the FROM clause (OR-combined).
    pub from: Vec<String>,
    pub completed: Option<bool>,
    pub text_contains: Vec<String>,
    /// True when the block used syntax outside the supported subset.
    pub unsupported: bool,
}

/// A todo collected from a note referenced by a dataview TASK query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalTodo {
    /// Vault-relative path of the note the todo lives in (slash-separated).
    pub source_note: String,
    pub item: TodoItem,
}

/// Parse all ```dataview fences whose body starts with `TASK`.
pub fn parse_task_queries(content: &str) -> Vec<TaskQuery> {
    let mut queries = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut fence_lines: Option<Vec<&str>> = None;
    let mut info = String::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        let fence_char = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        let run = fence_char
            .map(|c| trimmed.chars().take_while(|t| *t == c).count())
            .unwrap_or(0);
        if run >= 3 {
            match fence {
                None => {
                    fence = Some((fence_char.unwrap(), run));
                    info = trimmed[run..].trim().to_string();
                    fence_lines = Some(Vec::new());
                    continue;
                }
                Some((marker, len)) if fence_char == Some(marker) && run >= len => {
                    if let Some(lines) = fence_lines.take() {
                        if info.eq_ignore_ascii_case("dataview") {
                            if let Some(query) = parse_query_body(&lines) {
                                queries.push(query);
                            }
                        }
                    }
                    fence = None;
                    continue;
                }
                _ => {}
            }
        }
        if let Some(lines) = fence_lines.as_mut() {
            lines.push(line);
        }
    }
    queries
}

fn parse_query_body(lines: &[&str]) -> Option<TaskQuery> {
    // Clauses may be spread across lines; dataview treats newlines as spaces.
    let body = lines
        .iter()
        .flat_map(|l| l.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ");
    if !body
        .strip_prefix("TASK")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    {
        return None;
    }
    // Drop the TASK keyword so only FROM/WHERE clauses remain.
    let clauses_body = body.strip_prefix("TASK").unwrap_or(&body);
    let mut query = TaskQuery {
        from: Vec::new(),
        completed: None,
        text_contains: Vec::new(),
        unsupported: false,
    };
    for clause in split_clauses(clauses_body) {
        if let Some(rest) = clause.strip_prefix("FROM") {
            parse_from(rest, &mut query);
        } else if let Some(rest) = clause.strip_prefix("WHERE") {
            if !parse_where(rest, &mut query) {
                query.unsupported = true;
            }
        } else {
            query.unsupported = true;
        }
    }
    if query.from.is_empty() && !query.unsupported {
        // Bare `TASK` means the whole vault, which we do not scan.
        query.unsupported = true;
    }
    Some(query)
}

/// Split a query into FROM/WHERE clauses: a keyword (FROM, WHERE, OR-joined
/// arguments stay attached to their clause).
fn split_clauses(body: &str) -> Vec<String> {
    let mut clauses: Vec<String> = Vec::new();
    for word in body.split_whitespace() {
        let upper = word.to_uppercase();
        if upper == "FROM" || upper == "WHERE" || clauses.is_empty() {
            clauses.push(word.to_string());
        } else {
            clauses.last_mut().expect("clauses non-empty").push(' ');
            clauses
                .last_mut()
                .expect("clauses non-empty")
                .push_str(word);
        }
    }
    clauses
}

/// Split a FROM argument list on case-insensitive ` OR ` separators.
fn split_or(rest: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut current = String::new();
    for word in rest.split_whitespace() {
        if word.eq_ignore_ascii_case("OR") && !current.is_empty() {
            targets.push(current.trim().to_string());
            current.clear();
        } else {
            current.push_str(word);
            current.push(' ');
        }
    }
    if !current.trim().is_empty() {
        targets.push(current.trim().to_string());
    }
    targets
}

fn parse_from(rest: &str, query: &mut TaskQuery) {
    for target in split_or(rest) {
        let target = target.trim();
        let Some(path) = target.strip_prefix('"').and_then(|t| t.strip_suffix('"')) else {
            // Tags (#tag), links ([[note]]), parentheses and combinations are
            // outside the supported subset.
            query.unsupported = true;
            continue;
        };
        let sanitized = sanitize_rel(path.trim().trim_end_matches(".md").to_string());
        if sanitized.is_empty() {
            query.unsupported = true;
            continue;
        }
        if !query.from.contains(&sanitized) {
            query.from.push(sanitized);
        }
    }
}

fn parse_where(rest: &str, query: &mut TaskQuery) -> bool {
    let rest = rest.trim();
    if rest == "completed" {
        if query.completed.is_none() {
            query.completed = Some(true);
            return true;
        }
        return false;
    }
    if rest == "!completed" {
        if query.completed.is_none() {
            query.completed = Some(false);
            return true;
        }
        return false;
    }
    if let Some(args) = rest
        .strip_prefix("contains(text,")
        .and_then(|t| t.strip_suffix(')'))
    {
        let arg = args.trim();
        if let Some(lit) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) {
            query.text_contains.push(lit.to_string());
            return true;
        }
    }
    false
}

/// Collect todos matching the dataview TASK queries in `content`, reading the
/// referenced notes under `vault`. Files are deduplicated across queries;
/// read errors and vault-escape attempts skip the offending file rather than
/// failing the whole snapshot.
pub fn collect_external_todos(
    vault: &Vault,
    content: &str,
) -> Result<Vec<ExternalTodo>, VaultError> {
    let mut todos = Vec::new();
    let mut seen_files = BTreeSet::new();
    for query in parse_task_queries(content) {
        if query.unsupported {
            continue;
        }
        for source in resolve_sources(vault, &query.from)? {
            if !seen_files.insert(source.clone()) {
                continue;
            }
            let Ok(body) = fs::read_to_string(&source) else {
                continue;
            };
            for item in parse_todos(&body) {
                if let Some(completed) = query.completed {
                    if item.checked != completed {
                        continue;
                    }
                }
                if !query
                    .text_contains
                    .iter()
                    .all(|needle| item.text.contains(needle.as_str()))
                {
                    continue;
                }
                let rel = source
                    .strip_prefix(&vault.root)
                    .unwrap_or(&source)
                    .to_string_lossy()
                    .replace('\\', "/");
                todos.push(ExternalTodo {
                    source_note: rel,
                    item,
                });
            }
        }
    }
    Ok(todos)
}

/// Resolve FROM targets to concrete note files. A target may be a folder
/// (scanned recursively for `*.md`) or a note (with or without `.md`).
fn resolve_sources(vault: &Vault, from: &[String]) -> Result<Vec<PathBuf>, VaultError> {
    let mut files = Vec::new();
    for target in from {
        let mut path = vault.root.clone();
        for part in target.split('/') {
            push_safe_component(&mut path, part)?;
        }
        if path.extension().is_none() {
            let as_note = {
                let mut p = path.clone();
                p.set_extension("md");
                p
            };
            if as_note.is_file() {
                path = as_note;
            }
        }
        ensure_under_root(&vault.root, &path)?;
        if path.is_dir() {
            collect_markdown(&path, &mut files)?;
            continue;
        }
        if path.is_file() {
            files.push(path);
        }
        // Missing targets contribute no tasks, matching dataview's empty result.
    }
    files.sort();
    Ok(files)
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), VaultError> {
    if out.len() >= MAX_FILES_PER_FOLDER {
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
            collect_markdown(&path, out)?;
        } else if name_str.ends_with(".md") {
            ensure_under_root(
                // The vault-root check for walked files: keep the original
                // root prefix guard by construction (we only descended from
                // an already-verified directory).
                dir, &path,
            )?;
            out.push(path);
            if out.len() >= MAX_FILES_PER_FOLDER {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn vault_with(content: &str) -> (Vault, String) {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "obsidian-daily-qs-dv-{}-{}-{}",
            std::process::id(),
            n,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("Daily")).unwrap();
        fs::create_dir_all(root.join("Projects")).unwrap();
        fs::write(root.join("Daily").join("2026-08-20.md"), content).unwrap();
        (Vault { root: root.clone() }, root.display().to_string())
    }

    #[test]
    fn parses_task_from_folder_query() {
        let queries = parse_task_queries("```dataview\nTASK FROM \"Projects\"\n```\n");
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].from, vec!["Projects".to_string()]);
        assert!(!queries[0].unsupported);
    }

    #[test]
    fn parses_task_from_or_list_with_where() {
        let queries = parse_task_queries(
            "```dataview\nTASK FROM \"Projects\" OR \"Work\"\nWHERE !completed\nWHERE contains(text, \"#task\")\n```\n",
        );
        assert_eq!(queries.len(), 1);
        assert_eq!(
            queries[0].from,
            vec!["Projects".to_string(), "Work".to_string()]
        );
        assert_eq!(queries[0].completed, Some(false));
        assert_eq!(queries[0].text_contains, vec!["#task".to_string()]);
    }

    #[test]
    fn ignores_non_dataview_and_non_task_blocks() {
        let queries = parse_task_queries(
            "```dataviewjs\ndv.taskList();\n```\n```dataview\nLIST FROM \"Projects\"\n```\n```rust\nlet x = 1;\n```\n",
        );
        assert!(queries.is_empty());
    }

    #[test]
    fn marks_unsupported_clauses() {
        let queries =
            parse_task_queries("```dataview\nTASK FROM \"Projects\"\nGROUP BY file.folder\n```\n");
        assert_eq!(queries.len(), 1);
        assert!(queries[0].unsupported);

        let bare = parse_task_queries("```dataview\nTASK\n```\n");
        assert_eq!(bare.len(), 1);
        assert!(bare[0].unsupported);

        let tags = parse_task_queries("```dataview\nTASK FROM #inbox\n```\n");
        assert_eq!(tags.len(), 1);
        assert!(tags[0].unsupported);
    }

    #[test]
    fn collects_todos_from_referenced_folder_with_filters() {
        let (vault, root) = vault_with(
            "```dataview\nTASK FROM \"Projects\" WHERE !completed WHERE contains(text, \"#task\")\n```\n",
        );
        fs::write(
            PathBuf::from(&root).join("Projects").join("alpha.md"),
            "- [ ] ship #task\n- [x] done #task\n- [ ] no tag\n",
        )
        .unwrap();
        fs::create_dir_all(PathBuf::from(&root).join("Projects").join("sub")).unwrap();
        fs::write(
            PathBuf::from(&root)
                .join("Projects")
                .join("sub")
                .join("beta.md"),
            "- [ ] nested #task\n",
        )
        .unwrap();
        let todos = collect_external_todos(&vault, "```dataview\nTASK FROM \"Projects\" WHERE !completed WHERE contains(text, \"#task\")\n```\n").unwrap();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].source_note, "Projects/alpha.md");
        assert_eq!(todos[0].item.text, "ship #task");
        assert_eq!(todos[1].source_note, "Projects/sub/beta.md");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn collects_from_single_note_target() {
        let (vault, root) = vault_with("");
        fs::write(
            PathBuf::from(&root).join("Projects").join("alpha.md"),
            "- [ ] one\n- [x] two\n",
        )
        .unwrap();
        let todos =
            collect_external_todos(&vault, "```dataview\nTASK FROM \"Projects/alpha\"\n```\n")
                .unwrap();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].source_note, "Projects/alpha.md");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_target_and_escape_attempts_are_safe() {
        let (vault, root) = vault_with("");
        let todos =
            collect_external_todos(&vault, "```dataview\nTASK FROM \"Nope\"\n```\n").unwrap();
        assert!(todos.is_empty());

        let escape = collect_external_todos(&vault, "```dataview\nTASK FROM \"../../etc\"\n```\n");
        // Sanitized to etc/ under the vault, which does not exist: no tasks.
        assert!(escape.unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dedupes_files_across_overlapping_queries() {
        let (vault, root) = vault_with("");
        fs::write(
            PathBuf::from(&root).join("Projects").join("alpha.md"),
            "- [ ] one\n",
        )
        .unwrap();
        let todos = collect_external_todos(
            &vault,
            "```dataview\nTASK FROM \"Projects\"\n```\n```dataview\nTASK FROM \"Projects/alpha\"\n```\n",
        )
        .unwrap();
        assert_eq!(todos.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
