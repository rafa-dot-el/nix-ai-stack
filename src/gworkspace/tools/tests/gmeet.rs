mod common;

use std::collections::BTreeMap;

use gworkspace::{config::Config, context::Context, meet::Server};
use gworkspace_auth::AuthConfig;
use rafa_config::SecretRef;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;

const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;

fn ctx(tag: &str, s: &common::Stub) -> (Context, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!("gmeet-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    std::fs::write(d.join("rt"), "rt").unwrap();
    let auth = AuthConfig {
        client: SecretRef::File(d.join("client.json").display().to_string()),
        refresh_token: SecretRef::File(d.join("rt").display().to_string()),
        store_refresh_token: None,
    };
    let endpoints = BTreeMap::from([
        ("token".to_string(), format!("{}/token", s.base)),
        ("meet".to_string(), format!("{}/v2", s.base)),
        ("calendar".to_string(), format!("{}/calendar/v3", s.base)),
        ("drive".to_string(), format!("{}/drive/v3/files", s.base)),
        ("docs".to_string(), format!("{}/v1/documents", s.base)),
    ]);
    std::fs::write(
        d.join("cfg/rafa/gworkspace.yaml"),
        format!(
            "auth:\n  client: {{ file: \"{0}/client.json\" }}\n  refresh_token: {{ file: \"{0}/rt\" }}\nendpoints:\n  token: \"{1}/token\"\n  meet: \"{1}/v2\"\n  calendar: \"{1}/calendar/v3\"\n",
            d.display(),
            s.base
        ),
    )
    .unwrap();
    (Context::new("gmeet", Config { auth: Some(auth), endpoints }), d)
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

#[tokio::test]
async fn tools_are_read_only() {
    let s = common::stub(vec![]).await;
    let (c, _) = ctx("ann", &s);
    let tools = agent(Server::new(c)).await.list_all_tools().await.unwrap();
    let mut names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(names, vec!["attachment", "get", "list", "occurrences", "participants", "recordings", "search_transcripts", "transcript"]);
    assert!(tools.iter().all(|t| t.annotations.as_ref().unwrap().read_only_hint == Some(true)));
}

#[tokio::test]
async fn list_get_and_transcripts() {
    let s = common::stub(vec![
        token(),
        (200, json!({"items": [{"id": "E", "summary": "Sync", "conferenceData": {"conferenceId": "abc-mnop-xyz"}}]})),
        (200, json!({"id": "E", "summary": "Sync", "description": "<i>x</i>"})),
        (200, json!({"transcripts": [{"name": "conferenceRecords/R/transcripts/T", "docsDestination": {"document": "D"}}]})),
        (200, json!({"documentId": "D", "body": {"content": [{"paragraph": {"elements": [{"textRun": {"content": "Ada: hi\n"}}]}}]}})),
    ])
    .await;
    let (c, _) = ctx("lg", &s);
    let a = agent(Server::new(c)).await;
    let (ok, v) = call(&a, "list", json!({"query": "Sync"})).await;
    assert!(ok, "{v}");
    assert_eq!(v[0]["meet_code"], "abc-mnop-xyz");
    assert_eq!(call(&a, "get", json!({"event_id": "E"})).await.1["description"], "x");
    assert_eq!(call(&a, "transcript", json!({"conference_record": "conferenceRecords/R", "style": "document"})).await.1, json!({"document_id": "D", "text": "Ada: hi\n"}));
    let q = &s.seen()[1].target;
    assert!(q.starts_with("/calendar/v3/calendars/primary/events?") && q.contains("q=Sync") && q.contains("maxResults=50"), "{q}");
}

#[tokio::test]
async fn bad_names_and_styles_are_refused() {
    let s = common::stub(vec![token()]).await;
    let (c, _) = ctx("bad", &s);
    let a = agent(Server::new(c)).await;
    assert!(!call(&a, "participants", json!({"conference_record": "conferenceRecords/../../x"})).await.0);
    let (ok, v) = call(&a, "transcript", json!({"conference_record": "conferenceRecords/R", "style": "poem"})).await;
    assert!(!ok && v.as_str().unwrap().contains("entries"), "{v}");
}

#[tokio::test]
async fn attachments_check_status_and_binaries_get_a_link() {
    let s = common::stub(vec![
        token(),
        (200, json!({"id": "F", "name": "notes.txt", "mimeType": "text/plain"})),
        (403, json!({"error": "forbidden"})),
        (200, json!({"id": "G", "name": "pic.png", "mimeType": "image/png", "webViewLink": "https://drive/x"})),
    ])
    .await;
    let (c, _) = ctx("att", &s);
    let a = agent(Server::new(c)).await;
    let (ok, v) = call(&a, "attachment", json!({"file_id": "F"})).await;
    assert!(!ok && v.as_str().unwrap().contains("403"), "an error body is not content: {v}");
    let (ok, v) = call(&a, "attachment", json!({"file_id": "G"})).await;
    assert!(ok && v["content"].is_null() && v["web_view_link"] == "https://drive/x", "{v}");
}

#[tokio::test]
async fn transcript_search_quotes_the_query() {
    let s = common::stub(vec![token(), (200, json!({"files": [{"id": "D", "name": "Sync - Transcript"}]}))]).await;
    let (c, _) = ctx("search", &s);
    let a = agent(Server::new(c)).await;
    let (ok, v) = call(&a, "search_transcripts", json!({"query": r"it's\"})).await;
    assert!(ok, "{v}");
    let target = &s.seen()[1].target;
    let q: BTreeMap<String, String> = url::Url::parse(&format!("http://h{target}")).unwrap().query_pairs().into_owned().collect();
    assert!(q["q"].contains(r"name contains 'it\'s\\'") && q["q"].contains("createdTime > '"), "{}", q["q"]);
}

#[test]
fn the_binary_lists_occurrences() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![token(), (200, json!({"conferenceRecords": [{"name": "conferenceRecords/R"}]})), (200, json!({"transcripts": []})), (200, json!({"recordings": []}))]));
    let (_, d) = ctx("bin", &s);
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_gmeet"));
    c.args(["occurrences", "abc-mnop-xyz", "--json"]).env_clear().env("HOME", &d).env("XDG_CONFIG_HOME", d.join("cfg"));
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("conferenceRecords/R"));
}
