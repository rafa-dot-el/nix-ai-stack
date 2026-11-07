use chrono::{TimeZone, Utc};
use claude_usage::{exporter::{label, metrics, serve, FETCH_FAILED_BODY}, model::UsageData};

fn usage(name: &str) -> UsageData {
    serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
}

#[test]
fn label_values_are_escaped() {
    // bug 9abbe18f
    assert_eq!(label(r#"Opus "4" \ x"#), r#"Opus \"4\" \\ x"#);
    assert_eq!(label("a\nb"), "a\\nb");
    assert_eq!(label("plain"), "plain");
}

#[test]
fn the_exposition_has_every_family_and_fetch_ok() {
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let m = metrics(&usage("usage_multi_model.json"), &now);
    for family in ["claude_usage_percent", "claude_reset_seconds", "claude_usage_severity", "claude_fetch_ok"] {
        assert!(m.contains(&format!("# TYPE {family} gauge")), "{family}");
    }
    assert!(m.ends_with("claude_fetch_ok 1\n"));
    assert!(m.contains("claude_usage_percent{window=\"five_hour\"}"), "{m}");
    assert!(m.lines().all(|l| l.is_empty() || l.starts_with('#') || l.starts_with("claude_")), "{m}");
}

#[test]
fn a_hostile_model_name_cannot_add_a_line() {
    let mut u = usage("usage_multi_model.json");
    let l = u.limits.as_mut().unwrap().iter_mut().find(|l| l.scope.is_some()).unwrap();
    l.scope.as_mut().unwrap().model.as_mut().unwrap().display_name = Some("x\"} 1\nevil_metric 99\n#".into());
    let m = metrics(&u, &Utc::now());
    assert!(!m.lines().any(|l| l.starts_with("evil_metric")), "{m}");
}

async fn get(addr: std::net::SocketAddr, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn the_listener_serves_the_path_404s_the_rest_and_reports_failures() {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let c = calls.clone();
    tokio::spawn(serve(l, "/metrics".into(), move || {
        let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move { if n == 0 { Ok("claude_fetch_ok 1\n".to_string()) } else { Err("offline".to_string()) } }
    }));
    let ok = get(addr, "/metrics").await;
    assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("text/plain; version=0.0.4") && ok.ends_with("claude_fetch_ok 1\n"), "{ok}");
    let failed = get(addr, "/metrics").await;
    assert!(failed.starts_with("HTTP/1.1 200") && failed.ends_with(FETCH_FAILED_BODY), "{failed}");
    assert!(get(addr, "/other").await.starts_with("HTTP/1.1 404"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2, "404s do not scrape");
}
