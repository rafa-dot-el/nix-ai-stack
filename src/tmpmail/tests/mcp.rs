mod common;

use std::{collections::BTreeMap, sync::Arc};

use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;
use tmpmail::{mcp::Server, setup::{load, Setup}};

fn env(tag: &str, endpoint: Option<&str>) -> (std::path::PathBuf, BTreeMap<String, String>) {
    let d = std::env::temp_dir().join(format!("tmpmail-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    if let Some(e) = endpoint {
        std::fs::write(d.join("cfg/rafa/tmpmail.yaml"), format!("version: 1\nendpoint: {e}\n")).unwrap();
    }
    let env = BTreeMap::from([("HOME".to_string(), d.display().to_string()), ("XDG_CONFIG_HOME".to_string(), d.join("cfg").display().to_string())]);
    (d, env)
}

#[test]
fn setup_paths_and_endpoint() {
    let (d, mut e) = env("setup", None);
    assert_eq!(load(&e).unwrap(), Setup { endpoint: tmpmail::api::BASE.into(), identities: d.join(".local/state/rafa/tmpmail/identities.yaml") });
    e.insert("XDG_STATE_HOME".into(), "/st".into());
    assert_eq!(load(&e).unwrap().identities, std::path::PathBuf::from("/st/rafa/tmpmail/identities.yaml"));
    std::fs::write(d.join("cfg/rafa/tmpmail.yaml"), "endpoint: http://127.0.0.1:9/ajax.php\n").unwrap();
    assert_eq!(load(&e).unwrap().endpoint, "http://127.0.0.1:9/ajax.php");
    std::fs::write(d.join("cfg/rafa/tmpmail.yaml"), "endpiont: x\n").unwrap();
    assert!(load(&e).unwrap_err().contains("endpiont"), "unknown keys are errors");
    std::fs::write(d.join("cfg/rafa/tmpmail.yaml"), "endpoint: 3\n").unwrap();
    assert!(load(&e).unwrap_err().contains("endpoint"));
}

async fn agent(server: Server) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (a, b) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(r) = server.serve(b).await {
            let _ = r.waiting().await;
        }
    });
    ().serve(a).await.unwrap()
}

async fn call(a: &rmcp::service::RunningService<rmcp::RoleClient, ()>, name: &str, args: serde_json::Value) -> (bool, String) {
    let mut p = CallToolRequestParams::new(name.to_string());
    p.arguments = args.as_object().cloned();
    let CallToolResponse::Complete(r) = a.call_tool_once(p).await.unwrap() else { panic!() };
    let text: String = r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect();
    (r.is_error != Some(true), text)
}

#[tokio::test]
async fn tools_are_annotated() {
    let (_, e) = env("names", None);
    let a = agent(Server::new(Arc::new(load(&e).unwrap()))).await;
    let tools = a.list_all_tools().await.unwrap();
    let ann = |n: &str| tools.iter().find(|t| t.name == n).unwrap_or_else(|| panic!("{n}")).annotations.clone().unwrap();
    for ro in ["list_identities", "list_emails", "get_email"] {
        assert_eq!(ann(ro).read_only_hint, Some(true), "{ro}");
    }
    assert_eq!((ann("new_identity").read_only_hint, ann("new_identity").destructive_hint), (Some(false), Some(false)));
    assert_eq!(ann("forget_identity").destructive_hint, Some(true));
    assert_eq!(tools.len(), 5);
    let get = tools.iter().find(|t| t.name == "get_email").unwrap();
    assert!(get.description.as_deref().unwrap().contains("untrusted"), "mail is third-party text");
}

#[tokio::test]
async fn a_whole_inbox_lifecycle_never_shows_the_token() {
    let s = common::stub(vec![
        (200, json!({"email_addr": "x@sharklasers.com", "sid_token": "SECRET-SID"})),
        (200, json!({"list": [{"mail_id": "5", "mail_from": "a@b", "mail_subject": "Code", "mail_date": "d", "mail_excerpt": "e", "mail_read": 0}]})),
        (200, json!({"mail_id": "5", "mail_from": "a@b", "mail_subject": "Code", "mail_date": "d", "mail_body": "<b>123456</b> &amp; more"})),
        (200, json!(true)),
    ])
    .await;
    let (d, e) = env("life", Some(&format!("{}/ajax.php", s.base)));
    let a = agent(Server::new(Arc::new(load(&e).unwrap()))).await;
    let mut all = String::new();
    for (name, args) in [("new_identity", json!({})), ("list_identities", json!({})), ("list_emails", json!({})), ("get_email", json!({"id": "5"})), ("forget_identity", json!({}))] {
        let (ok, text) = call(&a, name, args).await;
        assert!(ok, "{name}: {text}");
        all.push_str(&text);
    }
    assert!(all.contains("x@sharklasers.com") && all.contains("\"id\": \"5\"") && all.contains("123456 & more"), "{all}");
    assert!(!all.contains("SECRET-SID"), "the token never leaves the file");
    assert!(s.seen()[2].target.contains("email_id=5"));
    let ids = std::fs::read_to_string(d.join(".local/state/rafa/tmpmail/identities.yaml")).unwrap();
    assert!(!ids.contains("x@sharklasers.com"), "forgotten: {ids}");
    let (ok, text) = call(&a, "list_emails", json!({})).await;
    assert!(!ok && text.contains("new-identity"), "{text}");
}
