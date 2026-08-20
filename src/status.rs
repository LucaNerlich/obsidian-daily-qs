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
            }],
        );
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["state"], "ok");
        assert_eq!(json["openCount"], 1);
        assert_eq!(json["doneCount"], 0);
        assert_eq!(json["todos"][0]["line"], 3);
    }

    #[test]
    fn serializes_error_snapshot() {
        let json = serde_json::to_string(&Snapshot::error("missing vault")).unwrap();
        assert!(json.contains(r#""state":"error""#));
        assert!(json.contains("missing vault"));
    }
}
