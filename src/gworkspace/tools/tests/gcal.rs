mod common;

use std::collections::BTreeMap;

use gworkspace::{calendar::{local_timezone, Server}, config::Config, context::Context};
use gworkspace_auth::AuthConfig;
use rafa_config::SecretRef;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;

const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;

fn home(tag: &str, s: &common::Stub) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gcal-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    std::fs::write(d.join("rt"), "rt").unwrap();
    std::fs::write(
        d.join("cfg/rafa/gworkspace.yaml"),
        format!("auth:\n  client: {{ file: \"{0}/client.json\" }}\n  refresh_token: {{ file: \"{0}/rt\" }}\nendpoints:\n  token: \"{1}/token\"\n  calendar: \"{1}/calendar/v3\"\n", d.display(), s.base),
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
    let endpoints = BTreeMap::from([("token".to_string(), format!("{}/token", s.base)), ("calendar".to_string(), format!("{}/calendar/v3", s.base))]);
    Context::new("gcal", Config { auth: Some(auth), endpoints })
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

fn events() -> serde_json::Value {
    json!({"items": [
        {"id": "e1", "summary": "Standup", "start": {"dateTime": "2026-06-01T09:00:00Z"}, "end": {"dateTime": "2026-06-01T09:15:00Z"}},
        {"id": "e2", "summary": "Holiday", "start": {"date": "2026-06-02"}, "end": {"date": "2026-06-03"}}
    ]})
}

#[test]
fn local_timezone_prefers_the_flag_then_tz() {
    assert_eq!(local_timezone(Some("Europe/Warsaw"), None).unwrap(), chrono_tz::Europe::Warsaw);
    assert_eq!(local_timezone(None, Some("Asia/Tokyo")).unwrap(), chrono_tz::Asia::Tokyo);
    assert!(local_timezone(Some("Nowhere/X"), None).is_err());
}

#[tokio::test]
async fn tools_are_read_only_and_list_get_download() {
    let s = common::stub(vec![token(), (200, events()), (200, json!({"id": "e1", "summary": "Standup", "start": {"dateTime": "2026-06-01T09:00:00Z"}})), (200, events())]).await;
    let d = home("tools", &s);
    let a = agent(Server::new(ctx(&d, &s))).await;
    let tools = a.list_all_tools().await.unwrap();
    let mut names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(names, vec!["download_ical", "get", "list"]);
    assert!(tools.iter().all(|t| t.annotations.as_ref().unwrap().read_only_hint == Some(true)));
    let (ok, out) = call(&a, "list", json!({"past_days": 1, "future_days": 1, "max_results": 1})).await;
    assert!(ok, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1, "max_results");
    assert_eq!(v[0]["id"], "e2", "all-day first");
    assert!(call(&a, "get", json!({"event_id": "e1"})).await.1.contains("Standup"));
    let (_, ics) = call(&a, "download_ical", json!({})).await;
    let v: serde_json::Value = serde_json::from_str(&ics).unwrap();
    assert!(v["ical"].as_str().unwrap().starts_with("BEGIN:VCALENDAR\r\n"));
    assert_eq!(v["events"], 2);
    assert!(s.seen()[1].target.starts_with("/calendar/v3/calendars/primary/events?"), "{}", s.seen()[1].target);
}

#[tokio::test]
async fn a_negative_window_is_refused() {
    let s = common::stub(vec![]).await;
    let d = home("neg", &s);
    let a = agent(Server::new(ctx(&d, &s))).await;
    let (ok, msg) = call(&a, "list", json!({"past_days": -1})).await;
    assert!(!ok && msg.contains("negative"), "{msg}");
}

fn bin(d: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_gcal"));
    c.args(args).env_clear().env("HOME", d).env("XDG_CONFIG_HOME", d.join("cfg")).env("TZ", "Europe/Warsaw");
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    c.output().unwrap()
}

#[test]
fn sync_writes_an_org_file_for_an_explicit_range() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![token(), (200, events())]));
    let d = home("sync", &s);
    let org = d.join("out/cal.org");
    let o = bin(&d, &["sync", "--output", org.to_str().unwrap(), "--since", "2026-06-01", "--until", "2026-06-30"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = std::fs::read_to_string(&org).unwrap();
    assert!(text.contains("* Standup") && text.contains("<2026-06-01 Mon 11:00-11:15>"), "in TZ: {text}");
    let q = &s.seen()[1].target;
    assert!(q.contains("timeMin=2026-05-31T22%3A00%3A00Z") && q.contains("timeMax=2026-06-30T22%3A00%3A00Z"), "{q}");
    assert!(!bin(&d, &["sync"]).status.success(), "--output is required");
}

#[test]
fn download_writes_ics() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![token(), (200, events())]));
    let d = home("ics", &s);
    let ics = d.join("cal.ics");
    let o = bin(&d, &["download", "--output", ics.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(std::fs::read_to_string(&ics).unwrap().contains("SUMMARY:Holiday\r\n"));
}
