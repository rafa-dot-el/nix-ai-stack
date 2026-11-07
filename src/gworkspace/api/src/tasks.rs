//! Google Tasks API v1: task lists and tasks; updates are GET-merge-PUT.
use serde::{Deserialize, Serialize};

use crate::{ApiError, Session};

pub const BASE: &str = "https://tasks.googleapis.com/tasks/v1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub notes: String,
    pub status: Option<String>,
    pub due: Option<String>,
    pub completed: Option<String>,
    pub parent: Option<String>,
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    pub id: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub notes: Option<String>,
    /// `Some("")` clears the due date.
    pub due: Option<String>,
    /// `needsAction` also clears `completed`.
    pub status: Option<String>,
}

/// Apply `update` to a task as the API returned it (pure).
pub fn merge(mut existing: serde_json::Value, update: &TaskUpdate) -> serde_json::Value {
    let Some(obj) = existing.as_object_mut() else { return existing };
    let s = |v: &str| serde_json::Value::String(v.to_string());
    if let Some(title) = &update.title {
        obj.insert("title".into(), s(title));
    }
    if let Some(notes) = &update.notes {
        obj.insert("notes".into(), s(notes));
    }
    match update.due.as_deref() {
        Some("") => {
            obj.remove("due");
        }
        Some(due) => {
            obj.insert("due".into(), s(due));
        }
        None => {}
    }
    if let Some(status) = &update.status {
        obj.insert("status".into(), s(status));
        if status == "needsAction" {
            obj.remove("completed");
        }
    }
    existing
}

/// Pick a list by exact id, else by case-insensitive title (pure).
pub fn resolve<'a>(lists: &'a [TaskList], id_or_title: &str) -> Result<&'a str, ApiError> {
    let by_id = lists.iter().find(|l| l.id.as_deref() == Some(id_or_title));
    let by_title = || lists.iter().find(|l| l.title.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(id_or_title)));
    if let Some(id) = by_id.or_else(by_title).and_then(|l| l.id.as_deref()) {
        return Ok(id);
    }
    let available: Vec<String> =
        lists.iter().map(|l| format!("{} ({})", l.title.as_deref().unwrap_or(""), l.id.as_deref().unwrap_or(""))).collect();
    Err(ApiError::Failed {
        api: "tasklists".into(),
        status: 404,
        body: format!("no task list with id or title {id_or_title:?}; available: {}", available.join(", ")),
    })
}

/// At most this many tasks per listing.
pub const LIST_CAP: usize = 100;

fn shape(raw: &serde_json::Value) -> Task {
    let field = |k: &str| raw.get(k).and_then(|v| v.as_str()).map(str::to_string);
    Task {
        id: field("id"),
        title: field("title"),
        notes: field("notes").unwrap_or_default(),
        status: field("status"),
        due: field("due"),
        completed: field("completed"),
        parent: field("parent"),
        updated: field("updated"),
    }
}

pub struct Tasks {
    pub session: Session,
}

impl Tasks {
    fn lists_url(&self) -> url::Url {
        self.session.url(&["users", "@me", "lists"])
    }

    pub async fn lists(&self) -> Result<Vec<TaskList>, ApiError> {
        let mut url = self.lists_url();
        url.query_pairs_mut().append_pair("maxResults", "100");
        let raw = self.session.paged(&url, "items", None).await?;
        raw.into_iter().map(|v| serde_json::from_value(v).map_err(|e| ApiError::Http(e.to_string()))).collect()
    }

    pub async fn resolve_list(&self, id_or_title: &str) -> Result<String, ApiError> {
        let lists = self.lists().await?;
        resolve(&lists, id_or_title).map(str::to_string)
    }

    pub async fn create_list(&self, title: &str) -> Result<TaskList, ApiError> {
        self.session.send(reqwest::Method::POST, &self.lists_url(), &serde_json::json!({ "title": title })).await
    }

    pub async fn delete_list(&self, list: &str) -> Result<(), ApiError> {
        self.session.delete(&self.session.url(&["users", "@me", "lists", list])).await
    }

    pub async fn tasks(&self, list: &str, show_completed: bool) -> Result<Vec<Task>, ApiError> {
        let mut url = self.session.url(&["lists", list, "tasks"]);
        url.query_pairs_mut()
            .append_pair("maxResults", "100")
            .append_pair("showCompleted", if show_completed { "true" } else { "false" })
            .append_pair("showHidden", "false");
        Ok(self.session.paged(&url, "items", Some(LIST_CAP)).await?.iter().map(shape).collect())
    }

    pub async fn get(&self, list: &str, task: &str) -> Result<Task, ApiError> {
        let raw: serde_json::Value = self.session.get(&self.session.url(&["lists", list, "tasks", task])).await?;
        Ok(shape(&raw))
    }

    pub async fn create(&self, list: &str, title: &str, notes: Option<&str>, due: Option<&str>, parent: Option<&str>) -> Result<Task, ApiError> {
        let mut body = serde_json::json!({ "title": title });
        if let Some(notes) = notes {
            body["notes"] = notes.into();
        }
        if let Some(due) = due {
            body["due"] = due.into();
        }
        let mut url = self.session.url(&["lists", list, "tasks"]);
        if let Some(parent) = parent {
            url.query_pairs_mut().append_pair("parent", parent);
        }
        let raw: serde_json::Value = self.session.send(reqwest::Method::POST, &url, &body).await?;
        Ok(shape(&raw))
    }

    pub async fn update(&self, list: &str, task: &str, update: &TaskUpdate) -> Result<Task, ApiError> {
        let url = self.session.url(&["lists", list, "tasks", task]);
        let existing: serde_json::Value = self.session.get(&url).await?;
        let raw: serde_json::Value = self.session.send(reqwest::Method::PUT, &url, &merge(existing, update)).await?;
        Ok(shape(&raw))
    }

    pub async fn delete(&self, list: &str, task: &str) -> Result<(), ApiError> {
        self.session.delete(&self.session.url(&["lists", list, "tasks", task])).await
    }
}
