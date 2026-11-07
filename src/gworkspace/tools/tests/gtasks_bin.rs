//! The built gtasks binary, env cleared, config pointing at a stub.
mod common;

use std::process::Command;

use rmcp::ServiceExt;
use serde_json::json;

const BIN: &str = env!("CARGO_BIN_EXE_gtasks");
const CLIENT: &str = r#"{"installed":{"client_id":"c","client_secret":"s","redirect_uris":["http://localhost:1"]}}"#;

struct Home(std::path::PathBuf);

impl Home {
    fn new(tag: &str, stub: Option<&common::Stub>) -> Home {
        let d = std::env::temp_dir().join(format!("gtasks-bin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
        if let Some(s) = stub {
            std::fs::write(d.join("client.json"), CLIENT).unwrap();
            std::fs::write(d.join("rt"), "rt").unwrap();
            let sink = d.join("stored");
            std::fs::write(
                d.join("cfg/rafa/gworkspace.yaml"),
                format!(
                    "version: 1\nauth:\n  client: {{ file: \"{0}/client.json\" }}\n  refresh_token: {{ file: \"{0}/rt\" }}\n  store_refresh_token: [sh, -c, \"cat > {1}\"]\nendpoints:\n  tasks: \"{2}/tasks/v1\"\n  token: \"{2}/token\"\n",
                    d.display(),
                    sink.display(),
                    s.base
                ),
            )
            .unwrap();
        }
        Home(d)
    }
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(BIN);
        c.args(args).env_clear().env("HOME", &self.0).env("XDG_CONFIG_HOME", self.0.join("cfg")).env("PATH", std::env::var("PATH").unwrap_or_default());
        // coverage profiles, and the trust store the Nix sandbox provides through cacert
        for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
            if let Ok(v) = std::env::var(var) {
                c.env(var, v);
            }
        }
        c
    }
}

fn run(c: &mut Command) -> (bool, String, String) {
    let o = c.output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn list_prints_human_and_json() {
    let s = common::stub(vec![
        (200, json!({"access_token": "AT", "expires_in": 3600})),
        (200, json!({"items": [{"id": "L1", "title": "Inbox"}]})),
        (200, json!({"access_token": "AT", "expires_in": 3600})),
        (200, json!({"items": [{"id": "L1", "title": "Inbox"}]})),
        (200, json!({"items": [{"id": "t", "title": "Do", "status": "needsAction"}]})),
    ])
    .await;
    let h = Home::new("list", Some(&s));
    let (ok, out, err) = tokio::task::spawn_blocking({ let mut c = h.cmd(&["list"]); move || run(&mut c) }).await.unwrap();
    assert!(ok, "{err}");
    assert_eq!(out, "1 task list(s):\n  Inbox (L1)\n");
    let (ok, out, err) = tokio::task::spawn_blocking({ let mut c = h.cmd(&["list", "--list", "inbox", "--json"]); move || run(&mut c) }).await.unwrap();
    assert!(ok, "{err}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap()[0]["title"], "Do");
}

#[tokio::test(flavor = "multi_thread")]
async fn create_update_delete_and_lists_from_the_cli() {
    let tok = || (200, json!({"access_token": "AT", "expires_in": 3600}));
    let lists = || (200, json!({"items": [{"id": "L1", "title": "Inbox"}]}));
    let s = common::stub(vec![
        tok(), lists(), (200, json!({"id": "n", "title": "T"})),
        tok(), lists(), (200, json!({"id": "n", "title": "T"})), (200, json!({"id": "n", "title": "U"})),
        tok(), lists(), (200, json!({"id": "n", "title": "U", "status": "needsAction"})),
        tok(), lists(), (204, json!(null)),
        tok(), (200, json!({"id": "L2", "title": "New"})),
        tok(), lists(), (204, json!(null)),
    ])
    .await;
    let h = Home::new("crud", Some(&s));
    for (args, expect) in [
        (vec!["create", "T", "--list", "Inbox", "--notes", "n"], "Created: T (n)"),
        (vec!["update", "n", "--list", "Inbox", "--title", "U"], "Updated: U (n)"),
        (vec!["get", "n", "--list", "Inbox", "--yaml"], "title: U"),
        (vec!["delete", "n", "--list", "Inbox"], "Deleted: n"),
        (vec!["create-list", "New"], "Created list: New (L2)"),
        (vec!["delete-list", "--list", "Inbox"], "Deleted list: L1"),
    ] {
        let (ok, out, err) = tokio::task::spawn_blocking({ let mut c = h.cmd(&args); move || run(&mut c) }).await.unwrap();
        assert!(ok && out.contains(expect), "{args:?}: {out} {err}");
    }
}

#[test]
fn status_and_token_without_configuration_fail_with_the_fix() {
    let h = Home::new("unconfigured", None);
    let (ok, out, err) = run(&mut h.cmd(&["status"]));
    assert!(!ok);
    assert!(format!("{out}{err}").contains("gworkspace.yaml"), "{out}{err}");
    assert!(!run(&mut h.cmd(&["token"])).0);
    let (ok, out, _) = run(&mut h.cmd(&["--help"]));
    assert!(ok && out.contains("serve") && out.contains("create-list"));
}

#[tokio::test(flavor = "multi_thread")]
async fn status_and_token_with_a_working_refresh() {
    let s = common::stub(vec![(200, json!({"access_token": "AT-1", "expires_in": 3600})), (200, json!({"access_token": "AT-2", "expires_in": 3600}))]).await;
    let h = Home::new("status", Some(&s));
    let (ok, out, err) = tokio::task::spawn_blocking({ let mut c = h.cmd(&["status"]); move || run(&mut c) }).await.unwrap();
    assert!(ok && out.contains("[+]"), "{out}{err}");
    let (ok, out, _) = tokio::task::spawn_blocking({ let mut c = h.cmd(&["token"]); move || run(&mut c) }).await.unwrap();
    assert!(ok);
    assert_eq!(out, "AT-2\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_prints_the_consent_url_and_stores_the_refresh_token() {
    let s = common::stub(vec![(200, json!({"access_token": "AT", "expires_in": 3600, "refresh_token": "NEW"}))]).await;
    let h = Home::new("auth", Some(&s));
    let (ok, out, err) = tokio::task::spawn_blocking({ let mut c = h.cmd(&["auth", "--redirect-url", "http://localhost:1/?code=C"]); move || run(&mut c) }).await.unwrap();
    assert!(ok, "{out}{err}");
    assert!(out.contains("https://accounts.google.com/o/oauth2/v2/auth?"), "{out}");
    assert_eq!(std::fs::read_to_string(h.0.join("stored")).unwrap(), "NEW\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_on_stdio_answers_initialize_without_credentials() {
    let h = Home::new("serve", None);
    let child = rmcp::transport::TokioChildProcess::new(tokio::process::Command::from(h.cmd(&["serve"]))).unwrap();
    let agent = ().serve(child).await.unwrap();
    let tools = agent.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 7);
    agent.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_on_a_socket() {
    let h = Home::new("socket", None);
    let sock = h.0.join("g.sock");
    let mut child = tokio::process::Command::from(h.cmd(&["serve", "--socket", sock.to_str().unwrap()])).kill_on_drop(true).spawn().unwrap();
    for _ in 0..100 {
        if tokio::net::UnixStream::connect(&sock).await.is_ok() { break }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    let agent = ().serve(tokio::net::UnixStream::connect(&sock).await.unwrap()).await.unwrap();
    assert_eq!(agent.list_all_tools().await.unwrap().len(), 7);
    agent.cancel().await.unwrap();
    unsafe { libc_kill(child.id().unwrap() as i32) };
    assert!(child.wait().await.unwrap().success());
}

unsafe fn libc_kill(pid: i32) {
    extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
    kill(pid, 15);
}

#[test]
fn no_trust_store_is_an_error_not_a_panic() {
    let s = tokio::runtime::Runtime::new().unwrap().block_on(common::stub(vec![]));
    let h = Home::new("nocerts", Some(&s));
    let o = h.cmd(&["status"]).env("SSL_CERT_FILE", "/nonexistent").env("SSL_CERT_DIR", "/nonexistent").output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(err.contains("[-] gtasks") && err.contains("certificates"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
}
