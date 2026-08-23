//! JSON snapshot types shared by the CLI and QML frontend.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TodoItem {
    /// 1-based source line number in the daily note.
    pub line: usize,
    pub checked: bool,
    pub text: String,
    /// Nesting level under its parent todo: 0 = top-level, 1 = first level
    /// of indentation, … Raw indentation is normalized so a todo never nests
    /// more than one level deeper than the todo before it.
    pub depth: usize,
    /// 1-based line of the nearest preceding todo at a shallower depth
    /// (the parent), if any.
    #[serde(rename = "parentLine", skip_serializing_if = "Option::is_none")]
    pub parent_line: Option<usize>,
    /// Vault-relative path of the note a dataview-derived todo lives in;
    /// None for todos from the daily note itself.
    #[serde(rename = "sourceNote", skip_serializing_if = "Option::is_none")]
    pub source_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    #[serde(rename = "openCount", skip_serializing_if = "Option::is_none")]
    pub open_count: Option<usize>,
    #[serde(rename = "doneCount", skip_serializing_if = "Option::is_none")]
    pub done_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub todos: Option<Vec<TodoItem>>,
    /// Absolute `obsidian://open?path=…` URI for the daily note path.
    #[serde(rename = "obsidianUri", skip_serializing_if = "Option::is_none")]
    pub obsidian_uri: Option<String>,
    /// Open todos on the previous calendar day (for carry-over UI).
    #[serde(rename = "carryOverCount", skip_serializing_if = "Option::is_none")]
    pub carry_over_count: Option<usize>,
    /// Todos matching ```dataview TASK queries in the daily note, living in
    /// other notes (`sourceNote` set on each item).
    #[serde(rename = "dataviewTodos", skip_serializing_if = "Option::is_none")]
    pub dataview_todos: Option<Vec<TodoItem>>,
    #[serde(rename = "dataviewOpenCount", skip_serializing_if = "Option::is_none")]
    pub dataview_open_count: Option<usize>,
    #[serde(rename = "dataviewDoneCount", skip_serializing_if = "Option::is_none")]
    pub dataview_done_count: Option<usize>,
    /// Read-only Tasks `happens on <panel date>` todos (e.g. `path includes Routines`).
    #[serde(rename = "tasksTodayTodos", skip_serializing_if = "Option::is_none")]
    pub tasks_today_todos: Option<Vec<TodoItem>>,
    #[serde(
        rename = "tasksTodayOpenCount",
        skip_serializing_if = "Option::is_none"
    )]
    pub tasks_today_open_count: Option<usize>,
    #[serde(
        rename = "tasksTodayDoneCount",
        skip_serializing_if = "Option::is_none"
    )]
    pub tasks_today_done_count: Option<usize>,
    #[serde(rename = "isToday", skip_serializing_if = "Option::is_none")]
    pub is_today: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Snapshot {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            state: State::Error,
            date: None,
            path: None,
            exists: None,
            open_count: None,
            done_count: None,
            todos: None,
            obsidian_uri: None,
            carry_over_count: None,
            dataview_todos: None,
            dataview_open_count: None,
            dataview_done_count: None,
            tasks_today_todos: None,
            tasks_today_open_count: None,
            tasks_today_done_count: None,
            is_today: None,
            error: Some(message.into()),
        }
    }

    pub fn ok(date: String, path: String, exists: bool, todos: Vec<TodoItem>) -> Self {
        let open_count = todos.iter().filter(|t| !t.checked).count();
        let done_count = todos.iter().filter(|t| t.checked).count();
        Self {
            state: State::Ok,
            date: Some(date),
            path: Some(path),
            exists: Some(exists),
            open_count: Some(open_count),
            done_count: Some(done_count),
            todos: Some(todos),
            obsidian_uri: None,
            carry_over_count: None,
            dataview_todos: None,
            dataview_open_count: None,
            dataview_done_count: None,
            tasks_today_todos: None,
            tasks_today_open_count: None,
            tasks_today_done_count: None,
            is_today: None,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_ok_snapshot() {
        let snap = Snapshot::ok(
            "2026-08-20".into(),
            "/vault/2026-08-20.md".into(),
            true,
            vec![TodoItem {
                line: 3,
                checked: false,
                text: "Ship".into(),
                depth: 0,
                parent_line: None,
                source_note: None,
            }],
        );
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["state"], "ok");
        assert_eq!(json["openCount"], 1);
        assert_eq!(json["doneCount"], 0);
        assert_eq!(json["todos"][0]["line"], 3);
    }

    #[test]
    fn serializes_nested_todos() {
        let snap = Snapshot::ok(
            "2026-08-20".into(),
            "/vault/2026-08-20.md".into(),
            true,
            vec![
                TodoItem {
                    line: 3,
                    checked: false,
                    text: "Parent".into(),
                    depth: 0,
                    parent_line: None,
                    source_note: None,
                },
                TodoItem {
                    line: 4,
                    checked: false,
                    text: "Child".into(),
                    depth: 1,
                    parent_line: Some(3),
                    source_note: None,
                },
            ],
        );
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["todos"][0]["depth"], 0);
        assert!(json["todos"][0].get("parentLine").is_none());
        assert_eq!(json["todos"][1]["depth"], 1);
        assert_eq!(json["todos"][1]["parentLine"], 3);
    }

    #[test]
    fn serializes_error_snapshot() {
        let json = serde_json::to_string(&Snapshot::error("missing vault")).unwrap();
        assert!(json.contains(r#""state":"error""#));
        assert!(json.contains("missing vault"));
    }
}
