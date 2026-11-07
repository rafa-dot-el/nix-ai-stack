mod common;

use std::process::Command;

use serde_json::json;

const BIN: &str = env!("CARGO_BIN_EXE_slack");

fn home(tag: &str, yaml: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("slack-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::create_dir_all(d.join("share")).unwrap();
    std::fs::write(d.join("token"), "xoxp-cli\n").unwrap();
    std::fs::write(d.join("cfg/rafa/slack.yaml"), yaml.replace("DIR", &d.display().to_string())).unwrap();
    d
}

fn run(d: &std::path::Path, base: &str, args: &[&str]) -> std::process::Output {
    let mut c = Command::new(BIN);
    c.args(args).env_clear().env("HOME", d).env("XDG_CONFIG_HOME", d.join("cfg")).env("SLACK_API_BASE", base);
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    c.output().unwrap()
}

const YAML: &str = "credentials:\n  token: { file: DIR/token }\npolicy:\n  rules:\n    - { match: { self_dm: true }, action: live }\nattachments: { allowed_roots: [DIR/share] }\n";

#[test]
fn send_flags_dry_run() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![(200, json!({"ok": true, "user_id": "UME"})), (200, json!({"ok": true, "channel": {"id": "DME"}}))]));
    let d = home("dry", YAML);
    std::fs::write(d.join("share/r.md"), "# Weekly\nAll green.").unwrap();
    let o = run(&d, &s.base, &["send", "--to", "me", "--markdown", d.join("share/r.md").to_str().unwrap(), "--no-preview", "--dry-run"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let post = &v["requests"][0]["chat.postMessage"];
    assert_eq!((post["markdown_text"].clone(), post["unfurl_links"].clone(), post["unfurl_media"].clone()), (json!("# Weekly\nAll green."), json!(false), json!(false)));
    assert_eq!(s.seen().len(), 2, "resolution only");
}

#[test]
fn send_from_yaml_with_flag_overrides_and_a_real_post() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![
        (200, json!({"ok": true, "user_id": "UME"})),
        (200, json!({"ok": true, "channel": {"id": "DME"}})),
        (200, json!({"ok": true, "ts": "5.5"})),
    ]));
    let d = home("yaml", YAML);
    std::fs::write(d.join("msg.yaml"), "to: \"#elsewhere\"\ntext: from the file\n").unwrap();
    let o = run(&d, &s.base, &["send", "--from-yaml", d.join("msg.yaml").to_str().unwrap(), "--to", "me"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("5.5"));
    let body: serde_json::Value = serde_json::from_str(&s.seen()[2].body).unwrap();
    assert_eq!((body["channel"].clone(), body["text"].clone()), (json!("DME"), json!("from the file")), "--to overrides the file's to");
}

#[test]
fn refusals_are_errors_with_exit_1_and_usage_is_exit_2() {
    let d = home("refuse", YAML);
    let o = run(&d, "http://127.0.0.1:1", &["send", "--to", "me", "--text", "x", "--draft", "--schedule", "2026-12-01 09:00"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("[-] slack"), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(run(&d, "http://127.0.0.1:1", &["send", "--bogus"]).status.code(), Some(2));
    let unconfigured = home("unconf", "");
    let o = run(&unconfigured, "http://127.0.0.1:1", &["status"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("\"configured\": false"), "{}", String::from_utf8_lossy(&o.stdout));
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_on_stdio() {
    use rmcp::ServiceExt;
    let d = home("serve", "");
    let mut c = tokio::process::Command::new(BIN);
    c.arg("serve").env_clear().env("HOME", &d).env("XDG_CONFIG_HOME", d.join("cfg"));
    if let Ok(v) = std::env::var("LLVM_PROFILE_FILE") {
        c.env("LLVM_PROFILE_FILE", v);
    }
    let agent = ().serve(rmcp::transport::TokioChildProcess::new(c).unwrap()).await.unwrap();
    assert_eq!(agent.list_all_tools().await.unwrap().len(), 28);
    agent.cancel().await.unwrap();
}
