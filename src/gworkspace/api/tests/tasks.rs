mod common;

use gworkspace_api::{tasks::{merge, resolve, TaskList, TaskUpdate, Tasks}, Session};
use serde_json::json;

fn lists() -> Vec<TaskList> {
    vec![TaskList { id: Some("L1".into()), title: Some("Inbox".into()) }, TaskList { id: Some("L2".into()), title: Some("Work".into()) }]
}

#[test]
fn resolve_by_id_then_case_insensitive_title() {
    assert_eq!(resolve(&lists(), "L2").unwrap(), "L2");
    assert_eq!(resolve(&lists(), "inbox").unwrap(), "L1");
    let err = resolve(&lists(), "Home").unwrap_err().to_string();
    assert!(err.contains("Home") && err.contains("Inbox (L1)") && err.contains("Work (L2)"), "{err}");
}

#[test]
fn merge_keeps_unknown_fields_and_applies_the_rules() {
    let existing = json!({"id": "t", "title": "old", "due": "2026-01-01T00:00:00Z", "status": "completed", "completed": "x", "etag": "e"});
    let m = merge(existing.clone(), &TaskUpdate { title: Some("new".into()), ..Default::default() });
    assert_eq!(m, json!({"id": "t", "title": "new", "due": "2026-01-01T00:00:00Z", "status": "completed", "completed": "x", "etag": "e"}));
    let m = merge(existing.clone(), &TaskUpdate { due: Some(String::new()), status: Some("needsAction".into()), notes: Some("n".into()), ..Default::default() });
    assert_eq!(m, json!({"id": "t", "title": "old", "status": "needsAction", "notes": "n", "etag": "e"}));
    let m = merge(existing, &TaskUpdate { status: Some("completed".into()), ..Default::default() });
    assert_eq!(m["completed"], "x", "completing keeps the timestamp");
}

fn tasks(base: &str) -> Tasks {
    Tasks { session: Session::new("tok", format!("{base}/tasks/v1")).unwrap() }
}

#[tokio::test]
async fn listing_tasks_pages_and_shapes_them() {
    let s = common::stub(vec![
        (200, json!({"items": [{"id": "a", "title": "A", "notes": "n"}], "nextPageToken": "p2"})),
        (200, json!({"items": [{"id": "b", "title": "B", "status": "needsAction"}]})),
    ])
    .await;
    let got = tasks(&s.base).tasks("L 1", false).await.unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!((got[0].id.as_deref(), got[0].notes.as_str(), got[1].notes.as_str()), (Some("a"), "n", ""));
    let first = &s.seen()[0].target;
    assert!(first.starts_with("/tasks/v1/lists/L%201/tasks?"), "{first}");
    assert!(first.contains("maxResults=100") && first.contains("showCompleted=false") && first.contains("showHidden=false"), "{first}");
}

#[tokio::test]
async fn resolve_list_asks_for_the_lists() {
    let s = common::stub(vec![(200, json!({"items": [{"id": "L1", "title": "Inbox"}]}))]).await;
    assert_eq!(tasks(&s.base).resolve_list("INBOX").await.unwrap(), "L1");
    assert!(s.seen()[0].target.starts_with("/tasks/v1/users/@me/lists?maxResults=100"), "{}", s.seen()[0].target);
}

#[tokio::test]
async fn create_with_parent_and_due() {
    let s = common::stub(vec![(200, json!({"id": "n", "title": "T"}))]).await;
    let t = tasks(&s.base).create("L1", "T", Some("notes"), Some("2026-12-01T00:00:00Z"), Some("P")).await.unwrap();
    assert_eq!(t.id.as_deref(), Some("n"));
    let seen = &s.seen()[0];
    assert_eq!((seen.method.as_str(), seen.target.as_str()), ("POST", "/tasks/v1/lists/L1/tasks?parent=P"));
    let body: serde_json::Value = serde_json::from_str(&seen.body).unwrap();
    assert_eq!(body, json!({"title": "T", "notes": "notes", "due": "2026-12-01T00:00:00Z"}));
}

#[tokio::test]
async fn update_gets_merges_and_puts() {
    let s = common::stub(vec![(200, json!({"id": "t", "title": "old", "etag": "e"})), (200, json!({"id": "t", "title": "new"}))]).await;
    let t = tasks(&s.base).update("L1", "t", &TaskUpdate { title: Some("new".into()), ..Default::default() }).await.unwrap();
    assert_eq!(t.title.as_deref(), Some("new"));
    let seen = s.seen();
    assert_eq!((seen[0].method.as_str(), seen[1].method.as_str()), ("GET", "PUT"));
    assert_eq!(seen[1].target, "/tasks/v1/lists/L1/tasks/t");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[1].body).unwrap(), json!({"id": "t", "title": "new", "etag": "e"}));
}

#[tokio::test]
async fn get_delete_and_list_management() {
    let s = common::stub(vec![
        (200, json!({"id": "t", "title": "T"})),
        (204, json!(null)),
        (200, json!({"id": "L9", "title": "New"})),
        (204, json!(null)),
    ])
    .await;
    let c = tasks(&s.base);
    assert_eq!(c.get("L1", "t").await.unwrap().title.as_deref(), Some("T"));
    c.delete("L1", "t").await.unwrap();
    assert_eq!(c.create_list("New").await.unwrap().id.as_deref(), Some("L9"));
    c.delete_list("L9").await.unwrap();
    let t: Vec<_> = s.seen().iter().map(|x| format!("{} {}", x.method, x.target)).collect();
    assert_eq!(t, vec!["GET /tasks/v1/lists/L1/tasks/t", "DELETE /tasks/v1/lists/L1/tasks/t", "POST /tasks/v1/users/@me/lists", "DELETE /tasks/v1/users/@me/lists/L9"]);
}
