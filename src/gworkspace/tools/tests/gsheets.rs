mod common;

use std::collections::BTreeMap;

use gworkspace::{config::Config, context::Context, sheets::Server};
use gworkspace_auth::AuthConfig;
use rafa_config::SecretRef;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;

const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;

fn ctx(s: &common::Stub, tag: &str) -> (Context, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!("gsheets-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    std::fs::write(d.join("rt"), "rt").unwrap();
    let auth = AuthConfig {
        client: SecretRef::File(d.join("client.json").display().to_string()),
        refresh_token: SecretRef::File(d.join("rt").display().to_string()),
        store_refresh_token: None,
    };
    let endpoints = BTreeMap::from([
        ("token".to_string(), format!("{}/token", s.base)),
        ("sheets".to_string(), format!("{}/v4/spreadsheets", s.base)),
        ("drive".to_string(), format!("{}/drive/v3/files", s.base)),
    ]);
    (Context::new("gsheets", Config { auth: Some(auth), endpoints }), d)
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
    (r.is_error != Some(true), r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect())
}

fn token() -> (u16, serde_json::Value) {
    (200, json!({"access_token": "AT", "expires_in": 3600}))
}

fn meta(tabs: &[&str]) -> serde_json::Value {
    let sheets: Vec<_> = tabs.iter().enumerate().map(|(i, t)| json!({"properties": {"sheetId": i, "title": t, "index": i}})).collect();
    json!({"spreadsheetId": "S", "properties": {"title": "Book"}, "sheets": sheets})
}

#[tokio::test]
async fn tools_and_annotations() {
    let a = agent(Server::new(Context::new("gsheets", Config { auth: None, endpoints: Default::default() }))).await;
    let mut seen: Vec<(String, bool, bool)> = a
        .list_all_tools()
        .await
        .unwrap()
        .iter()
        .map(|t| {
            let an = t.annotations.as_ref().unwrap();
            (t.name.to_string(), an.read_only_hint == Some(true), an.destructive_hint == Some(true))
        })
        .collect();
    seen.sort();
    let expect = [
        ("add_sheet", false, false), ("append_rows", false, false), ("batch_read", true, false), ("delete_sheet", false, true),
        ("delete_spreadsheet", false, true), ("download_spreadsheet", true, false), ("get_spreadsheet", true, false), ("new", false, false),
        ("read_range", true, false), ("search_spreadsheets", true, false), ("write_range", false, false),
    ];
    assert_eq!(seen, expect.iter().map(|(n, r, d)| (n.to_string(), *r, *d)).collect::<Vec<_>>());
}

#[tokio::test]
async fn read_write_and_download() {
    let s = common::stub(vec![
        token(),
        (200, json!({"range": "A1", "values": [["1"]]})),
        (200, json!({"updatedCells": 1})),
        (200, meta(&["Only"])),
        (200, json!({"values": [["a", "b,c"]]})),
        (200, meta(&["One", "Two"])),
        (200, json!({"values": [["1"]]})),
        (200, json!({"values": []})),
    ])
    .await;
    let (c, _) = ctx(&s, "rw");
    let a = agent(Server::new(c)).await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&call(&a, "read_range", json!({"spreadsheet_id": "S", "range": "A1"})).await.1).unwrap()["values"], json!([["1"]]));
    assert!(call(&a, "write_range", json!({"spreadsheet_id": "S", "range": "A1", "values": [["=1"]]})).await.0);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&call(&a, "download_spreadsheet", json!({"spreadsheet_id": "S"})).await.1).unwrap(), json!({"single_csv": "a,\"b,c\"\r\n"}));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&call(&a, "download_spreadsheet", json!({"spreadsheet_id": "S"})).await.1).unwrap(),
        json!({"tabs": [{"title": "One", "csv": "1\r\n"}, {"title": "Two", "csv": ""}]})
    );
    assert!(s.seen()[2].target.ends_with("?valueInputOption=USER_ENTERED"), "the default");
}

#[tokio::test]
async fn value_input_option_is_validated_before_any_request() {
    let s = common::stub(vec![]).await;
    let (c, _) = ctx(&s, "vio");
    let a = agent(Server::new(c)).await;
    let (ok, msg) = call(&a, "write_range", json!({"spreadsheet_id": "S", "range": "A1", "values": [[1]], "value_input_option": "EVAL"})).await;
    assert!(!ok && msg.contains("USER_ENTERED") && msg.contains("RAW"), "{msg}");
    assert!(s.seen().is_empty());
}

#[tokio::test]
async fn tabs_new_search_delete() {
    let s = common::stub(vec![
        token(),
        (200, json!({"spreadsheetId": "N", "properties": {"title": "New"}, "spreadsheetUrl": "u"})),
        (200, json!({"replies": [{"addSheet": {"properties": {"sheetId": 4, "title": "T", "index": 1}}}]})),
        (200, json!({})),
        (200, json!({"files": [{"id": "S", "name": "Book"}]})),
        (200, json!({"id": "S", "name": "Book"})),
    ])
    .await;
    let (c, _) = ctx(&s, "tabs");
    let a = agent(Server::new(c)).await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&call(&a, "new", json!({"title": "New", "sheet_titles": ["A"]})).await.1).unwrap(), json!({"spreadsheet_id": "N", "title": "New", "url": "u"}));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&call(&a, "add_sheet", json!({"spreadsheet_id": "S", "title": "T"})).await.1).unwrap(), json!({"sheet_id": 4, "title": "T", "index": 1}));
    assert!(call(&a, "delete_sheet", json!({"spreadsheet_id": "S", "sheet_id": 4})).await.0);
    assert!(call(&a, "search_spreadsheets", json!({"query": "Bo"})).await.1.contains("Book"));
    assert!(call(&a, "delete_spreadsheet", json!({"spreadsheet_id": "S"})).await.0);
    assert!(s.seen()[4].target.contains("spreadsheet"), "the search is limited to spreadsheets");
    assert_eq!(s.seen()[5].method, "PATCH", "trash by default");
}

#[test]
fn the_binary_downloads_tabs_to_safe_files() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![token(), (200, meta(&["../evil", "ok"])), (200, json!({"values": [["x"]]})), (200, json!({"values": [["y"]]}))]));
    let (_, d) = ctx(&s, "bin");
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(
        d.join("cfg/rafa/gworkspace.yaml"),
        format!("auth:\n  client: {{ file: \"{0}/client.json\" }}\n  refresh_token: {{ file: \"{0}/rt\" }}\nendpoints:\n  token: \"{1}/token\"\n  sheets: \"{1}/v4/spreadsheets\"\n", d.display(), s.base),
    )
    .unwrap();
    let out = d.join("export");
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_gsheets"));
    c.args(["download", "S", "--output", out.to_str().unwrap()]).env_clear().env("HOME", &d).env("XDG_CONFIG_HOME", d.join("cfg"));
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let mut files: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, vec!["_evil.csv", "ok.csv"]);
    assert!(!d.join("evil.csv").exists());
}
