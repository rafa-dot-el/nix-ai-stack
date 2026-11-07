mod common;

use std::collections::BTreeMap;

use gworkspace::{config::Config, context::Context, drive::Server};
use gworkspace_auth::AuthConfig;
use rafa_config::SecretRef;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;

const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;
const FOLDER: &str = "application/vnd.google-apps.folder";

fn home(tag: &str, s: &common::Stub) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gdrive-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    std::fs::write(d.join("rt"), "rt").unwrap();
    std::fs::write(
        d.join("cfg/rafa/gworkspace.yaml"),
        format!("auth:\n  client: {{ file: \"{0}/client.json\" }}\n  refresh_token: {{ file: \"{0}/rt\" }}\nendpoints:\n  token: \"{1}/token\"\n  drive: \"{1}/drive/v3/files\"\n", d.display(), s.base),
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
    let endpoints = BTreeMap::from([("token".to_string(), format!("{}/token", s.base)), ("drive".to_string(), format!("{}/drive/v3/files", s.base))]);
    Context::new("gdrive", Config { auth: Some(auth), endpoints })
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

fn f(id: &str, name: &str, mime: &str, size: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "mimeType": mime, "size": size, "parents": ["P"]})
}

#[tokio::test]
async fn tools_and_annotations() {
    let s = common::stub(vec![]).await;
    let d = home("ann", &s);
    let a = agent(Server::new(ctx(&d, &s))).await;
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
        ("delete", false, true), ("download", true, false), ("get", true, false), ("list", true, false), ("move_file", false, false),
        ("search", true, false), ("trash", false, true), ("upload", false, false), ("usage", true, false),
    ];
    assert_eq!(seen, expect.iter().map(|(n, r, x)| (n.to_string(), *r, *x)).collect::<Vec<_>>());
}

#[tokio::test]
async fn download_is_text_when_it_can_be_and_base64_otherwise() {
    let s = common::stub(vec![token(), (200, f("1", "a.txt", "text/plain", "5")), (200, json!("hello")), (200, f("2", "b.bin", "application/octet-stream", "1"))]).await;
    let d = home("dl", &s);
    let a = agent(Server::new(ctx(&d, &s))).await;
    assert_eq!(call(&a, "download", json!({"file_id": "1"})).await.1, json!({"file_id": "1", "name": "a.txt", "text": "\"hello\""}));
    let (ok, v) = call(&a, "download", json!({"file_id": "2"})).await;
    assert!(!ok || v.get("base64_content").is_some() || v.get("text").is_some(), "{v}");
}

#[tokio::test]
async fn list_by_path_upload_usage_and_conflicts() {
    let s = common::stub(vec![
        token(),
        (200, json!({"files": [{"id": "DIR", "name": "Work"}]})),
        (200, json!({"files": [f("1", "a", "text/plain", "3")]})),
        (200, f("N", "n.txt", "text/plain", "2")),
        (200, json!({"files": [f("1", "a", "text/plain", "3"), f("D", "d", FOLDER, "0")]})),
        (200, json!({"files": [f("2", "b", "text/plain", "4")]})),
        (200, json!({"storageQuota": {"usage": "9", "usageInDrive": "8", "usageInDriveTrash": "1"}})),
    ])
    .await;
    let d = home("misc", &s);
    let a = agent(Server::new(ctx(&d, &s))).await;
    let (ok, v) = call(&a, "list", json!({"path": "Work"})).await;
    assert!(ok, "{v}");
    assert_eq!(v[0]["id"], "1");
    assert_eq!(call(&a, "upload", json!({"name": "n.txt", "mime_type": "text/plain", "content": "hi", "parent_id": "DIR"})).await.1["id"], "N");
    assert_eq!(call(&a, "usage", json!({"folder_id": "X"})).await.1, json!({"folder_id": "X", "file_count": 2, "total_bytes": 7}));
    assert_eq!(call(&a, "usage", json!({"trash": true})).await.1, json!({"usage_in_drive_trash": 1}));
    let (ok, v) = call(&a, "usage", json!({"trash": true, "folder_id": "X"})).await;
    assert!(!ok && v.as_str().unwrap().contains("both"));
    let (ok, v) = call(&a, "list", json!({"path": "a", "parent_id": "b"})).await;
    assert!(!ok && v.as_str().unwrap().contains("either"));
}

fn bin(home: &std::path::Path, cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_gdrive"));
    c.args(args).current_dir(cwd).env_clear().env("HOME", home).env("XDG_CONFIG_HOME", home.join("cfg"));
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    c.output().unwrap()
}

#[test]
fn folder_downloads_cannot_escape_the_output_directory() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![
        token(),
        (200, f("ROOT", "/tmp/absolute-escape", FOLDER, "0")),
        (200, json!({"files": [f("x", "../../escaped.txt", "text/plain", "1"), f("S", "..", FOLDER, "0")]})),
        (200, json!({"files": [f("y", "inner.txt", "text/plain", "1")]})),
        (200, json!("X")),
        (200, json!("Y")),
    ]));
    let d = home("trav", &s);
    let work = d.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let o = bin(&d, &work, &["download", "ROOT"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!d.join("escaped.txt").exists() && !std::path::Path::new("/tmp/absolute-escape").exists());
    let mut found: Vec<String> = walk(&work).into_iter().map(|p| p.strip_prefix(&work).unwrap().display().to_string()).collect();
    found.sort();
    assert_eq!(found, vec!["_tmp_absolute-escape/S/inner.txt", "_tmp_absolute-escape/_.._escaped.txt"]);
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
    }
    out
}
