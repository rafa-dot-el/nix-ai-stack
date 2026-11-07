//! gdocs tools and the built binary against a stub.
mod common;

use std::collections::BTreeMap;

use gworkspace::{config::Config, context::Context, docs::Server};
use gworkspace_auth::AuthConfig;
use rafa_config::SecretRef;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;

const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gdocs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    std::fs::write(d.join("rt"), "rt").unwrap();
    d
}

fn ctx(s: &common::Stub, d: &std::path::Path) -> Context {
    let auth = AuthConfig {
        client: SecretRef::File(d.join("client.json").display().to_string()),
        refresh_token: SecretRef::File(d.join("rt").display().to_string()),
        store_refresh_token: None,
    };
    let endpoints = BTreeMap::from([
        ("token".to_string(), format!("{}/token", s.base)),
        ("docs".to_string(), format!("{}/v1/documents", s.base)),
        ("drive".to_string(), format!("{}/drive/v3/files", s.base)),
    ]);
    Context::new("gdocs", Config { auth: Some(auth), endpoints })
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

async fn ok(a: &rmcp::service::RunningService<rmcp::RoleClient, ()>, name: &str, args: serde_json::Value) -> serde_json::Value {
    let mut p = CallToolRequestParams::new(name.to_string());
    p.arguments = args.as_object().cloned();
    let CallToolResponse::Complete(r) = a.call_tool_once(p).await.unwrap() else { panic!() };
    let text: String = r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect();
    assert_ne!(r.is_error, Some(true), "{name}: {text}");
    serde_json::from_str(&text).unwrap()
}

fn token() -> (u16, serde_json::Value) {
    (200, json!({"access_token": "AT", "expires_in": 3600}))
}

fn doc() -> serde_json::Value {
    json!({"documentId": "D", "title": "T", "body": {"content": [
        {"paragraph": {"paragraphStyle": {"namedStyleType": "HEADING_1"}, "elements": [{"textRun": {"content": "Head\n"}}]}},
        {"paragraph": {"elements": [{"textRun": {"content": "Text\n"}}], "paragraphStyle": {"namedStyleType": "NORMAL_TEXT"}}, "endIndex": 11}
    ]}})
}

#[tokio::test]
async fn tools_and_their_annotations() {
    let a = agent(Server::new(Context::new("gdocs", Config { auth: None, endpoints: Default::default() }))).await;
    let mut seen: Vec<(String, Option<bool>, Option<bool>)> = a
        .list_all_tools()
        .await
        .unwrap()
        .iter()
        .map(|t| (t.name.to_string(), t.annotations.as_ref().and_then(|x| x.read_only_hint), t.annotations.as_ref().and_then(|x| x.destructive_hint)))
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        vec![
            ("append_text".into(), Some(false), Some(false)),
            ("delete_document".into(), Some(false), Some(true)),
            ("download_document".into(), Some(true), Some(false)),
            ("get_document".into(), Some(true), Some(false)),
            ("get_document_structure".into(), Some(true), Some(false)),
            ("new_document".into(), Some(false), Some(false)),
            ("search_documents".into(), Some(true), Some(false)),
        ]
    );
}

#[tokio::test]
async fn read_tools() {
    let s = common::stub(vec![token(), (200, doc()), (200, doc()), (200, json!({"files": [{"id": "D", "name": "T"}]})), (200, json!(null))]).await;
    let a = agent(Server::new(ctx(&s, &scratch("read")))).await;
    assert_eq!(ok(&a, "get_document", json!({"document_id": "D"})).await, json!({"document_id": "D", "title": "T", "text": "Head\nText\n"}));
    assert_eq!(ok(&a, "get_document_structure", json!({"document_id": "D"})).await["text"], "# Head\nText\n");
    assert_eq!(ok(&a, "search_documents", json!({"query": "T"})).await, json!([{"id": "D", "name": "T", "modified_time": null}]));
    let q = &s.seen()[3].target;
    assert!(q.contains("pageSize=20"), "default max_results 20: {q}");
}

#[tokio::test]
async fn download_returns_markdown_text_directly() {
    let s = common::stub(vec![token(), (200, json!("# Head"))]).await;
    let a = agent(Server::new(ctx(&s, &scratch("md")))).await;
    let v = ok(&a, "download_document", json!({"document_id": "D", "markdown": true})).await;
    assert_eq!(v, json!({"document_id": "D", "format": "markdown", "text": "\"# Head\""}));
    assert_eq!(s.seen()[1].target, "/drive/v3/files/D/export?mimeType=text%2Fmarkdown");
}

#[tokio::test]
async fn write_tools() {
    let s = common::stub(vec![
        token(),
        (200, json!({"documentId": "N", "title": "New"})),
        (200, doc()),
        (200, json!({})),
        (200, json!({"id": "N", "name": "New"})),
        (204, json!(null)),
    ])
    .await;
    let a = agent(Server::new(ctx(&s, &scratch("write")))).await;
    assert_eq!(ok(&a, "new_document", json!({"title": "New"})).await["document_id"], "N");
    assert_eq!(ok(&a, "append_text", json!({"document_id": "D", "text": "more"})).await, json!({"document_id": "D", "start_index": 10, "end_index": 14}));
    assert_eq!(ok(&a, "delete_document", json!({"document_id": "N"})).await, json!({"document_id": "N", "permanent": false}));
    assert_eq!(ok(&a, "delete_document", json!({"document_id": "N", "permanent": true})).await["permanent"], true);
    let methods: Vec<_> = s.seen().iter().skip(4).map(|x| x.method.clone()).collect();
    assert_eq!(methods, vec!["PATCH", "DELETE"], "trash by default, delete when permanent");
}

#[test]
fn the_binary_reads_and_downloads() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![token(), (200, doc()), token(), (200, doc())]));
    let d = scratch("bin");
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(
        d.join("cfg/rafa/gworkspace.yaml"),
        format!("auth:\n  client: {{ file: \"{0}/client.json\" }}\n  refresh_token: {{ file: \"{0}/rt\" }}\nendpoints:\n  token: \"{1}/token\"\n  docs: \"{1}/v1/documents\"\n", d.display(), s.base),
    )
    .unwrap();
    let run = |args: &[&str]| {
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_gdocs"));
        c.args(args).env_clear().env("HOME", &d).env("XDG_CONFIG_HOME", d.join("cfg"));
        for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
            if let Ok(v) = std::env::var(var) {
                c.env(var, v);
            }
        }
        c.output().unwrap()
    };
    let o = run(&["get-document-structure", "D"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "# Head\nText\n\n");
    let out = d.join("doc.txt");
    let o = run(&["download", "D", "--output", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "Head\nText\n");
    let o = run(&["--help"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("search-documents"));
}
