//! gtasks against a stub of the token endpoint and the Tasks API.
mod common;

use std::collections::BTreeMap;

use gworkspace::{config::Config, context::not_configured, tasks::{human_lists, human_tasks, Context, Server}};
use gworkspace_api::tasks::{Task, TaskList};
use gworkspace_auth::AuthConfig;
use rafa_config::SecretRef;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;

const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;

fn ctx(stub: &common::Stub, dir: &std::path::Path) -> Context {
    std::fs::write(dir.join("client.json"), CLIENT).unwrap();
    std::fs::write(dir.join("rt"), "rt").unwrap();
    let auth = AuthConfig {
        client: SecretRef::File(dir.join("client.json").display().to_string()),
        refresh_token: SecretRef::File(dir.join("rt").display().to_string()),
        store_refresh_token: None,
    };
    let endpoints = BTreeMap::from([("tasks".to_string(), format!("{}/tasks/v1", stub.base)), ("token".to_string(), format!("{}/token", stub.base))]);
    Context::new("gtasks", Config { auth: Some(auth), endpoints })
}

fn scratch() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gtasks-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn agent(server: Server) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (a, b) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(b).await {
            let _ = running.waiting().await;
        }
    });
    ().serve(a).await.unwrap()
}

fn call(name: &str, args: serde_json::Value) -> CallToolRequestParams {
    let mut p = CallToolRequestParams::new(name.to_string());
    p.arguments = args.as_object().cloned();
    p
}

fn text(r: &rmcp::model::CallToolResult) -> String {
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect()
}

#[test]
fn human_output() {
    let lists = vec![TaskList { id: Some("L1".into()), title: Some("Inbox".into()) }];
    assert_eq!(human_lists(&lists), "1 task list(s):\n  Inbox (L1)");
    let tasks = vec![Task { id: Some("t".into()), title: Some("Do".into()), status: Some("needsAction".into()), ..Default::default() }];
    assert_eq!(human_tasks(&tasks), "1 task(s):\n  [needsAction] Do (t)");
}

#[tokio::test]
async fn tools_are_annotated_for_mcp_filters() {
    let tools = agent(Server::new(Context::new("gtasks", Config { auth: None, endpoints: Default::default() }))).await.list_all_tools().await.unwrap();
    let mut seen: Vec<(String, Option<bool>, Option<bool>)> = tools
        .iter()
        .map(|t| (t.name.to_string(), t.annotations.as_ref().and_then(|a| a.read_only_hint), t.annotations.as_ref().and_then(|a| a.destructive_hint)))
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        vec![
            ("create".into(), Some(false), Some(false)),
            ("create_list".into(), Some(false), Some(false)),
            ("delete".into(), Some(false), Some(true)),
            ("delete_list".into(), Some(false), Some(true)),
            ("get".into(), Some(true), Some(false)),
            ("list".into(), Some(true), Some(false)),
            ("update".into(), Some(false), Some(false)),
        ]
    );
}

#[tokio::test]
async fn without_credentials_every_tool_says_how_to_configure() {
    let a = agent(Server::new(Context::new("gtasks", Config { auth: None, endpoints: Default::default() }))).await;
    let CallToolResponse::Complete(r) = a.call_tool_once(call("list", json!({}))).await.unwrap() else { panic!() };
    assert_eq!(r.is_error, Some(true));
    assert_eq!(text(&r), not_configured("gtasks"));
}

#[tokio::test]
async fn list_resolves_a_title_and_returns_json() {
    let s = common::stub(vec![
        (200, json!({"access_token": "AT", "expires_in": 3600})),
        (200, json!({"items": [{"id": "L1", "title": "Inbox"}]})),
        (200, json!({"items": [{"id": "t1", "title": "Do", "status": "needsAction"}]})),
    ])
    .await;
    let a = agent(Server::new(ctx(&s, &scratch()))).await;
    let CallToolResponse::Complete(r) = a.call_tool_once(call("list", json!({"list": "inbox"}))).await.unwrap() else { panic!() };
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let v: serde_json::Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(v[0]["id"], "t1");
    let seen = s.seen();
    assert_eq!(seen[0].target, "/token");
    assert_eq!(seen[2].auth, "Bearer AT");
    assert!(seen[2].target.starts_with("/tasks/v1/lists/L1/tasks?"), "{}", seen[2].target);
}

#[tokio::test]
async fn list_without_a_list_returns_the_lists_and_respects_max_results() {
    let s = common::stub(vec![
        (200, json!({"access_token": "AT", "expires_in": 3600})),
        (200, json!({"items": [{"id": "L1", "title": "A"}, {"id": "L2", "title": "B"}]})),
    ])
    .await;
    let a = agent(Server::new(ctx(&s, &scratch()))).await;
    let CallToolResponse::Complete(r) = a.call_tool_once(call("list", json!({"max_results": 1}))).await.unwrap() else { panic!() };
    let v: serde_json::Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(v, json!([{"id": "L1", "title": "A"}]));
}

#[tokio::test]
async fn create_update_delete_through_tools() {
    let s = common::stub(vec![
        (200, json!({"access_token": "AT", "expires_in": 3600})),
        (200, json!({"items": [{"id": "L1", "title": "Inbox"}]})),
        (200, json!({"id": "n", "title": "T"})),
        (200, json!({"items": [{"id": "L1", "title": "Inbox"}]})),
        (200, json!({"id": "n", "title": "T"})),
        (200, json!({"id": "n", "title": "T", "status": "completed"})),
        (200, json!({"items": [{"id": "L1", "title": "Inbox"}]})),
        (204, json!(null)),
    ])
    .await;
    let a = agent(Server::new(ctx(&s, &scratch()))).await;
    let ok = |r: CallToolResponse| match r {
        CallToolResponse::Complete(r) if r.is_error != Some(true) => text(&r),
        other => panic!("{other:?}"),
    };
    assert!(ok(a.call_tool_once(call("create", json!({"list": "L1", "title": "T", "due": "2026-12-01T00:00:00Z"}))).await.unwrap()).contains("\"n\""));
    assert!(ok(a.call_tool_once(call("update", json!({"list": "Inbox", "task_id": "n", "status": "completed"}))).await.unwrap()).contains("completed"));
    assert!(ok(a.call_tool_once(call("delete", json!({"list": "Inbox", "task_id": "n"}))).await.unwrap()).contains("deleted"));
    assert_eq!(s.seen().iter().filter(|x| x.target == "/token").count(), 1, "one token for all calls");
}

#[tokio::test]
async fn api_errors_become_tool_errors() {
    let s = common::stub(vec![(200, json!({"access_token": "AT", "expires_in": 3600})), (403, json!({"error": {"message": "insufficient scope"}}))]).await;
    let a = agent(Server::new(ctx(&s, &scratch()))).await;
    let CallToolResponse::Complete(r) = a.call_tool_once(call("get", json!({"list": "L1", "task_id": "x"}))).await.unwrap() else { panic!() };
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("403") && text(&r).contains("insufficient scope"), "{}", text(&r));
}
