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
    #[serde(rename = "isToday", skip_serializing_if = "Option::is_none")]
    pub is_today: Option<bool>,
    /// Relative template path from daily-notes.json when configured.
    #[serde(rename = "templateName", skip_serializing_if = "Option::is_none")]
    pub template_name: Option<String>,
    /// True when a template is configured and the note exists.
    #[serde(
        rename = "createdFromTemplate",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_from_template: Option<bool>,
    /// Machine-stable error class for UI empty states (`missing_vault`, …).
    #[serde(rename = "errorCode", skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DaySummary {
    pub date: String,
    #[serde(rename = "openCount")]
    pub open_count: usize,
    #[serde(rename = "doneCount")]
    pub done_count: usize,
    pub exists: bool,
    #[serde(rename = "isToday")]
    pub is_today: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeekSummary {
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<Vec<DaySummary>>,
    #[serde(rename = "errorCode", skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Snapshot {
    pub fn error(message: impl Into<String>) -> Self {
        Self::error_with_code(message, "io")
    }

    pub fn error_with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
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
            is_today: None,
            template_name: None,
            created_from_template: None,
            error_code: Some(code.into()),
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
            is_today: None,
            template_name: None,
            created_from_template: None,
            error_code: None,
            error: None,
        }
    }
}

impl WeekSummary {
    pub fn error(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            state: State::Error,
            days: None,
            error_code: Some(code.into()),
            error: Some(message.into()),
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
                },
                TodoItem {
                    line: 4,
                    checked: false,
                    text: "Child".into(),
                    depth: 1,
                    parent_line: Some(3),
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
