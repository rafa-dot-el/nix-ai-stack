mod common;

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_claude-usage");

fn home(tag: &str, endpoint: &str, creds: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cu-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::create_dir_all(d.join(".claude")).unwrap();
    std::fs::copy(format!("{}/tests/fixtures/{creds}", env!("CARGO_MANIFEST_DIR")), d.join(".claude/.credentials.json")).unwrap();
    std::fs::write(d.join("cfg/rafa/claude-usage.yaml"), format!("endpoint: {endpoint}\nexporter: {{ port: 0 }}\n")).unwrap();
    d
}

fn run(d: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut c = Command::new(BIN);
    c.args(args).env_clear().env("HOME", d).env("XDG_CONFIG_HOME", d.join("cfg")).env("TZ", "UTC");
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    c.output().unwrap()
}

#[test]
fn format_json_and_table() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let body: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/usage_full.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
    let s = rt.block_on(common::stub(vec![(200, body.clone()), (200, body.clone()), (200, body)]));
    let d = home("ok", &format!("{}/u", s.base), "credentials_full.json");
    let o = run(&d, &["--format", "{PCT_5H}%|{PLAN}", "--color-mode", "none"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).trim_end().ends_with("%|pro"), "{}", String::from_utf8_lossy(&o.stdout));
    let o = run(&d, &["--json"]);
    assert!(serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap().get("five_hour").is_some());
    let o = run(&d, &[]);
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("Claude Code Usage"), "not a terminal: no colour");
    assert_eq!(s.seen()[0].auth, "Bearer test-access-token-abc123");
}

#[test]
fn an_expired_token_is_exit_1_with_the_fix_and_no_request() {
    let d = home("exp", "http://127.0.0.1:1/u", "credentials_expired.json");
    let before = std::fs::read(d.join(".claude/.credentials.json")).unwrap();
    let o = run(&d, &["--json"]);
    assert_eq!(o.status.code(), Some(1));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.starts_with("[-] claude-usage: ") && err.contains("Claude Code"), "{err}");
    assert_eq!(std::fs::read(d.join(".claude/.credentials.json")).unwrap(), before);
    assert_eq!(run(&d, &["--bogus"]).status.code(), Some(2));
}
