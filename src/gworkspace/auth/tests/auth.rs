use std::{
    sync::{atomic::{AtomicUsize, Ordering}, Arc},
    time::{Duration, Instant},
};

use gworkspace_auth::*;
use rafa_config::{Layer, SecretRef};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

const CLIENT: &str = r#"{"installed":{"client_id":"cid","client_secret":"csecret","redirect_uris":["http://localhost:1","http://localhost:2"]}}"#;

fn client() -> OAuthClient {
    parse_client(CLIENT).unwrap()
}

#[test]
fn auth_config_takes_secret_refs_and_a_store_command() {
    let l = Layer::parse("g", "auth:\n  client: { file: /run/c.json }\n  refresh_token: { command: [pass, show, g/rt] }\n  store_refresh_token: [pass, insert, -mf, g/rt]\n").unwrap();
    let c = auth_config(l.value.get("auth")).unwrap();
    assert_eq!(c.client, SecretRef::File("/run/c.json".into()));
    assert_eq!(c.refresh_token, SecretRef::Command(vec!["pass".into(), "show".into(), "g/rt".into()]));
    assert_eq!(c.store_refresh_token, Some(vec!["pass".into(), "insert".into(), "-mf".into(), "g/rt".into()]));
}

#[test]
fn auth_config_errors_name_their_key() {
    assert!(auth_config(None).unwrap_err().to_string().contains("auth"));
    let inline = Layer::parse("g", "auth:\n  client: hunter2\n  refresh_token: { file: /x }\n").unwrap();
    assert!(auth_config(inline.value.get("auth")).unwrap_err().to_string().contains("auth.client"));
    let store = Layer::parse("g", "auth:\n  client: { file: /a }\n  refresh_token: { file: /x }\n  store_refresh_token: pass\n").unwrap();
    assert!(auth_config(store.value.get("auth")).unwrap_err().to_string().contains("store_refresh_token"));
}

#[test]
fn client_documents_are_validated() {
    assert_eq!(client(), OAuthClient { client_id: "cid".into(), client_secret: "csecret".into(), redirect_uris: vec!["http://localhost:1".into(), "http://localhost:2".into()] });
    for bad in ["{}", "nope", r#"{"installed":{"client_id":" ","client_secret":"s","redirect_uris":["x"]}}"#, r#"{"installed":{"client_id":"c","client_secret":"","redirect_uris":["x"]}}"#, r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":[]}}"#] {
        assert!(matches!(parse_client(bad), Err(AuthError::InvalidClient(_))), "{bad}");
    }
}

#[test]
fn consent_url_asks_offline_consent_for_every_scope() {
    let u = url::Url::parse(&consent_url(&client(), COMBINED_SCOPES)).unwrap();
    let q: std::collections::BTreeMap<_, _> = u.query_pairs().into_owned().collect();
    assert_eq!(u.as_str().split('?').next().unwrap(), AUTH_ENDPOINT);
    assert_eq!(q["client_id"], "cid");
    assert_eq!(q["redirect_uri"], "http://localhost:1");
    assert_eq!(q["access_type"], "offline");
    assert_eq!(q["prompt"], "consent");
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["scope"].split(' ').count(), COMBINED_SCOPES.len());
    assert!(!COMBINED_SCOPES.iter().any(|s| s.contains("keep")));
}

#[test]
fn codes_come_from_the_redirect_url() {
    assert_eq!(extract_code("  http://localhost:1/?code=abc&scope=x \n").unwrap(), "abc");
    assert!(matches!(extract_code("http://localhost:1/?error=denied"), Err(AuthError::MissingCode)));
    assert!(matches!(extract_code("http://localhost:1/?code="), Err(AuthError::MissingCode)));
    assert!(matches!(extract_code("not a url"), Err(AuthError::MissingCode)));
}

#[test]
fn forms() {
    let f = code_form(&client(), "abc", "http://localhost:2");
    assert_eq!(f, vec![("code", "abc".into()), ("client_id", "cid".into()), ("client_secret", "csecret".into()), ("redirect_uri", "http://localhost:2".into()), ("grant_type", "authorization_code".into())]);
    let r = refresh_form(&client(), "rt");
    assert_eq!(r, vec![("grant_type", "refresh_token".into()), ("client_id", "cid".into()), ("client_secret", "csecret".into()), ("refresh_token", "rt".into())]);
}

#[test]
fn cache_entries_expire_a_minute_early() {
    let now = Instant::now();
    let r = TokenResponse { access_token: "a".into(), expires_in: Some(3600), refresh_token: None, scope: None };
    assert_eq!(cache_entry(&r, now).until, now + Duration::from_secs(3540));
    let short = TokenResponse { expires_in: Some(30), ..r.clone() };
    assert_eq!(cache_entry(&short, now).until, now, "never negative");
    let none = TokenResponse { expires_in: None, ..r };
    assert_eq!(cache_entry(&none, now).until, now + Duration::from_secs(3540), "Google's default hour");
}

/// A one-endpoint HTTP stub: answers every POST with `body` under `status`, counts requests,
/// and records the last request body.
async fn token_stub(status: u16, body: &'static str) -> (String, Arc<AtomicUsize>, Arc<std::sync::Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    let (hits, last) = (Arc::new(AtomicUsize::new(0)), Arc::new(std::sync::Mutex::new(String::new())));
    let (h, l) = (hits.clone(), last.clone());
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            *l.lock().unwrap() = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            h.fetch_add(1, Ordering::SeqCst);
            let reply = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            s.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (url, hits, last)
}

fn config(dir: &std::path::Path, store: Option<Vec<String>>) -> AuthConfig {
    std::fs::write(dir.join("client.json"), CLIENT).unwrap();
    std::fs::write(dir.join("rt"), "the-refresh-token\n").unwrap();
    AuthConfig {
        client: SecretRef::File(dir.join("client.json").display().to_string()),
        refresh_token: SecretRef::File(dir.join("rt").display().to_string()),
        store_refresh_token: store,
    }
}

#[tokio::test]
async fn access_tokens_are_refreshed_once_and_cached() {
    let d = tempfile_dir();
    let (url, hits, last) = token_stub(200, r#"{"access_token":"at-1","expires_in":3600}"#).await;
    let auth = Auth::new(config(&d, None), url).unwrap();
    assert_eq!(auth.access_token().await.unwrap(), "at-1");
    assert_eq!(auth.access_token().await.unwrap(), "at-1");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "the second call is served from the cache");
    let body = last.lock().unwrap().clone();
    assert!(body.contains("grant_type=refresh_token") && body.contains("refresh_token=the-refresh-token"), "{body}");
}

#[tokio::test]
async fn token_endpoint_errors_carry_status_and_body() {
    let d = tempfile_dir();
    let (url, _, _) = token_stub(400, r#"{"error":"invalid_grant"}"#).await;
    let err = Auth::new(config(&d, None), url).unwrap().access_token().await.unwrap_err();
    assert!(matches!(err, AuthError::Token { status: 400, ref body } if body.contains("invalid_grant")), "{err}");
}

#[tokio::test]
async fn exchange_stores_the_refresh_token_through_the_command() {
    let d = tempfile_dir();
    let (url, _, last) = token_stub(200, r#"{"access_token":"at","expires_in":3600,"refresh_token":"new-rt"}"#).await;
    let sink = d.join("stored");
    let store = vec!["sh".into(), "-c".into(), format!("cat > {}", sink.display())];
    let auth = Auth::new(config(&d, Some(store)), url).unwrap();
    let t = auth.exchange("http://localhost:1/?code=the-code").await.unwrap();
    assert_eq!(t.refresh_token.as_deref(), Some("new-rt"));
    assert_eq!(std::fs::read_to_string(&sink).unwrap(), "new-rt\n");
    assert!(last.lock().unwrap().contains("code=the-code"));
    assert_eq!(auth.access_token().await.unwrap(), "at", "the exchanged access token is cached");
}

#[tokio::test]
async fn exchange_without_a_refresh_token_or_a_failing_store_is_an_error() {
    let d = tempfile_dir();
    let (url, _, _) = token_stub(200, r#"{"access_token":"at"}"#).await;
    assert!(matches!(Auth::new(config(&d, None), url).unwrap().exchange("http://x/?code=c").await, Err(AuthError::MissingRefreshToken)));
    let (url, _, _) = token_stub(200, r#"{"access_token":"at","refresh_token":"r"}"#).await;
    let failing = Some(vec!["false".to_string()]);
    assert!(matches!(Auth::new(config(&d, failing), url).unwrap().exchange("http://x/?code=c").await, Err(AuthError::Store(_))));
}

fn tempfile_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gw-auth-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rand_suffix() -> u64 {
    static N: AtomicUsize = AtomicUsize::new(0);
    N.fetch_add(1, Ordering::SeqCst) as u64 * 1_000_003 + std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as u64
}

#[tokio::test]
async fn command_secrets_are_run_and_their_failures_reported() {
    let d = tempfile_dir();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    let (url, _, last) = token_stub(200, r#"{"access_token":"at","expires_in":3600}"#).await;
    let ok = AuthConfig {
        client: SecretRef::File(d.join("client.json").display().to_string()),
        refresh_token: SecretRef::Command(vec!["sh".into(), "-c".into(), "printf from-command".into()]),
        store_refresh_token: None,
    };
    assert_eq!(Auth::new(ok, url.clone()).unwrap().access_token().await.unwrap(), "at");
    assert!(last.lock().unwrap().contains("refresh_token=from-command"));
    let failing = AuthConfig {
        client: SecretRef::File(d.join("client.json").display().to_string()),
        refresh_token: SecretRef::Command(vec!["sh".into(), "-c".into(), "echo nope >&2; exit 4".into()]),
        store_refresh_token: None,
    };
    let err = Auth::new(failing, url.clone()).unwrap().access_token().await.unwrap_err().to_string();
    assert!(err.contains("auth.refresh_token") && err.contains("nope"), "{err}");
    let missing = AuthConfig {
        client: SecretRef::File(d.join("client.json").display().to_string()),
        refresh_token: SecretRef::Command(vec!["/nonexistent/secret-tool".into()]),
        store_refresh_token: None,
    };
    assert!(Auth::new(missing, url).unwrap().access_token().await.unwrap_err().to_string().contains("/nonexistent/secret-tool"));
}

#[tokio::test]
async fn exchange_tries_each_redirect_uri_and_reports_the_last_failure() {
    let d = tempfile_dir();
    std::fs::write(d.join("client.json"), CLIENT).unwrap();
    std::fs::write(d.join("rt"), "rt").unwrap();
    // one listener answering 400 then 200
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    tokio::spawn(async move {
        for (status, body) in [(400, r#"{"error":"redirect_uri_mismatch"}"#), (200, r#"{"access_token":"at","refresh_token":"r"}"#), (400, "{}"), (400, r#"{"error":"last"}"#)] {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = s.read(&mut buf).await.unwrap();
            let reply = format!("HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            s.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    let auth = Auth::new(config(&d, None), url).unwrap();
    assert_eq!(auth.exchange("http://localhost:2/?code=c").await.unwrap().refresh_token.as_deref(), Some("r"));
    let err = auth.exchange("http://localhost:2/?code=c").await.unwrap_err();
    assert!(matches!(err, AuthError::Token { status: 400, ref body } if body.contains("last")), "{err}");
}
