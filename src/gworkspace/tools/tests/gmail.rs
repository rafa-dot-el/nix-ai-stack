mod common;

use std::collections::BTreeMap;

use base64::Engine;
use gworkspace::{config::Config, context::Context, mail::Server};
use gworkspace_auth::AuthConfig;
use rafa_config::SecretRef;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;

const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;

fn b64(s: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
}

fn home(tag: &str, s: &common::Stub) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gmail-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    std::fs::write(d.join("rt"), "rt").unwrap();
    std::fs::write(
        d.join("cfg/rafa/gworkspace.yaml"),
        format!("auth:\n  client: {{ file: \"{0}/client.json\" }}\n  refresh_token: {{ file: \"{0}/rt\" }}\nendpoints:\n  token: \"{1}/token\"\n  gmail: \"{1}/gmail/v1/users/me\"\n", d.display(), s.base),
    )
    .unwrap();
    d
}

fn ctx(d: &std::path::Path, s: &common::Stub) -> Context {
    let auth = AuthConfig {
        client: SecretRef::File(d.join("client.json").display().to_string()),
        refresh_token: SecretRef::File(d.join("rt").display().to_string()),
        store_refresh_token: None,
    };
    let endpoints = BTreeMap::from([("token".to_string(), format!("{}/token", s.base)), ("gmail".to_string(), format!("{}/gmail/v1/users/me", s.base))]);
    Context::new("gmail", Config { auth: Some(auth), endpoints })
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

async fn call(a: &rmcp::service::RunningService<rmcp::RoleClient, ()>, name: &str, args: serde_json::Value) -> (bool, serde_json::Value) {
    let mut p = CallToolRequestParams::new(name.to_string());
    p.arguments = args.as_object().cloned();
    let CallToolResponse::Complete(r) = a.call_tool_once(p).await.unwrap() else { panic!() };
    let text: String = r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect();
    (r.is_error != Some(true), serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))
}

fn token() -> (u16, serde_json::Value) {
    (200, json!({"access_token": "AT", "expires_in": 3600}))
}

fn message() -> serde_json::Value {
    json!({"id": "m1", "threadId": "t1", "payload": {"mimeType": "multipart/mixed", "headers": [{"name": "Subject", "value": "S"}], "parts": [
        {"mimeType": "text/plain", "body": {"data": b64(b"body")}},
        {"mimeType": "application/pdf", "filename": "../../evil.pdf", "body": {"attachmentId": "A1", "size": 3}}]}})
}

#[tokio::test]
async fn tools_and_annotations() {
    let s = common::stub(vec![]).await;
    let a = agent(Server::new(ctx(&home("ann", &s), &s))).await;
    let tools = a.list_all_tools().await.unwrap();
    let mut seen: Vec<(String, bool)> = tools.iter().map(|t| (t.name.to_string(), t.annotations.as_ref().unwrap().read_only_hint == Some(true))).collect();
    seen.sort();
    let expect = [
        ("create_draft", false), ("create_label", false), ("download_message", true), ("get", true), ("get_attachment", true), ("list", true),
        ("list_drafts", true), ("list_labels", true), ("modify_message_labels", false), ("modify_thread_labels", false), ("search", true),
    ];
    assert_eq!(seen, expect.iter().map(|(n, r)| (n.to_string(), *r)).collect::<Vec<_>>());
    let download = tools.iter().find(|t| t.name == "download_message").unwrap();
    assert!(!download.input_schema.get("properties").unwrap().as_object().unwrap().contains_key("output_dir"), "the MCP tool never writes files");
}

#[tokio::test]
async fn download_and_attachments_are_returned_inline() {
    let s = common::stub(vec![token(), (200, message()), (200, json!({"raw": b64(b"Subject: S\r\n\r\nbody")})), (200, json!({"data": b64(&[0xff, 0x00, 0x01])})), (200, json!({"data": b64(b"text attachment")}))]).await;
    let a = agent(Server::new(ctx(&home("inline", &s), &s))).await;
    let (ok, v) = call(&a, "download_message", json!({"message_id": "m1"})).await;
    assert!(ok, "{v}");
    assert_eq!(v["eml"], "Subject: S\r\n\r\nbody");
    assert_eq!(v["attachments"][0]["filename"], "../../evil.pdf");
    assert_eq!(call(&a, "get_attachment", json!({"message_id": "m1", "attachment_id": "A1"})).await.1, json!({"base64_content": base64::engine::general_purpose::STANDARD.encode([0xff, 0x00, 0x01])}));
    assert_eq!(call(&a, "get_attachment", json!({"message_id": "m1", "attachment_id": "A2"})).await.1, json!({"text": "text attachment"}));
}

#[tokio::test]
async fn injected_headers_are_refused_before_any_request() {
    let s = common::stub(vec![]).await;
    let a = agent(Server::new(ctx(&home("inj", &s), &s))).await;
    let (ok, v) = call(&a, "create_draft", json!({"to": "b@y.org\r\nBcc: spy@evil.org", "subject": "s", "body": "b"})).await;
    assert!(!ok && v.as_str().unwrap().contains("line break"), "{v}");
    assert!(s.seen().is_empty());
}

#[tokio::test]
async fn list_is_the_inbox_and_search_takes_a_query() {
    let s = common::stub(vec![token(), (200, json!({"threads": [{"id": "t1"}]})), (200, json!({"threads": []}))]).await;
    let a = agent(Server::new(ctx(&home("list", &s), &s))).await;
    assert_eq!(call(&a, "list", json!({})).await.1, json!([{"id": "t1", "snippet": null}]));
    call(&a, "search", json!({"query": "from:x"})).await;
    let t: Vec<_> = s.seen().iter().map(|x| x.target.clone()).collect();
    assert!(t[1].contains("q=in%3Ainbox") && t[1].contains("maxResults=20"), "{t:?}");
    assert!(t[2].contains("q=from%3Ax"), "{t:?}");
}

#[test]
fn the_binary_keeps_attachments_inside_the_output_directory() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![token(), (200, message()), (200, json!({"raw": b64(b"Subject: S\r\n\r\nbody")})), (200, json!({"data": b64(b"PDF")}))]));
    let d = home("bin", &s);
    let out = d.join("export");
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_gmail"));
    c.args(["download", "m1", "--output", out.to_str().unwrap()]).env_clear().env("HOME", &d).env("XDG_CONFIG_HOME", d.join("cfg"));
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_to_string(out.join("message.eml")).unwrap(), "Subject: S\r\n\r\nbody");
    assert_eq!(std::fs::read_to_string(out.join("attachments/_.._evil.pdf")).unwrap(), "PDF");
    assert!(!d.join("evil.pdf").exists());
}
