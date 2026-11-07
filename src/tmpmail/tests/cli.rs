mod common;

use std::process::Command;

use serde_json::json;

const BIN: &str = env!("CARGO_BIN_EXE_tmpmail");

fn home(tag: &str, endpoint: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tmpmail-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(d.join("cfg/rafa/tmpmail.yaml"), format!("endpoint: {endpoint}\n")).unwrap();
    d
}

fn run(d: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut c = Command::new(BIN);
    c.args(args).env_clear().env("HOME", d).env("XDG_CONFIG_HOME", d.join("cfg"));
    for var in ["LLVM_PROFILE_FILE", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Ok(v) = std::env::var(var) {
            c.env(var, v);
        }
    }
    c.output().unwrap()
}

#[test]
fn new_then_list_in_each_format() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(common::stub(vec![(200, json!({"email_addr": "x@grr.la", "sid_token": "SID"}))]));
    let d = home("new", &format!("{}/ajax.php", s.base));
    let o = run(&d, &["new-identity"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "x@grr.la\n");
    let o = run(&d, &["list-identities", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v[0]["email"], "x@grr.la");
    assert!(v[0].get("sid_token").is_none());
    assert!(String::from_utf8_lossy(&run(&d, &["list-identities", "--format", "yaml"]).stdout).contains("email: x@grr.la"));
}

#[test]
fn errors_exit_1_with_the_reason_and_usage_exits_2() {
    let d = home("err", "http://127.0.0.1:1/ajax.php");
    let o = run(&d, &["list-emails"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("[-] tmpmail: "), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(run(&d, &["bogus"]).status.code(), Some(2));
    assert_eq!(run(&d, &["list-identities", "--format", "xml"]).status.code(), Some(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_on_stdio() {
    use rmcp::ServiceExt;
    let d = home("serve", "http://127.0.0.1:1/ajax.php");
    let mut c = tokio::process::Command::new(BIN);
    c.arg("serve").env_clear().env("HOME", &d).env("XDG_CONFIG_HOME", d.join("cfg"));
    if let Ok(v) = std::env::var("LLVM_PROFILE_FILE") {
        c.env("LLVM_PROFILE_FILE", v);
    }
    let agent = ().serve(rmcp::transport::TokioChildProcess::new(c).unwrap()).await.unwrap();
    assert_eq!(agent.list_all_tools().await.unwrap().len(), 5);
    agent.cancel().await.unwrap();
}
