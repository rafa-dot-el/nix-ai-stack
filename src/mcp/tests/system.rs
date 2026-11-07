use std::{collections::BTreeMap, time::Duration};

use mcp::system::{activated_fd, config_home, load, notify, DirWatch, LoadError};

fn env(home: &std::path::Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".into(), home.display().to_string()),
        ("XDG_CONFIG_HOME".into(), home.join("cfg").display().to_string()),
        ("XDG_RUNTIME_DIR".into(), "/run/user/7".into()),
    ])
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn config_home_prefers_xdg() {
    let mut e = BTreeMap::from([("HOME".to_string(), "/h".to_string())]);
    assert_eq!(config_home(&e), "/h/.config");
    e.insert("XDG_CONFIG_HOME".into(), "/x".into());
    assert_eq!(config_home(&e), "/x");
}

#[test]
fn no_files_means_defaults_expanded() {
    let d = tempfile::tempdir().unwrap();
    let e = env(d.path());
    let c = load(&e, None).unwrap();
    assert_eq!(c.proxy.socket_path(&e).unwrap(), std::path::PathBuf::from("/run/user/7/rafa/mcp/proxy.sock"));
    assert_eq!(c.proxy.pool_path(&e).unwrap(), std::path::PathBuf::from("/run/user/7/rafa/mcp/servers"));
}

#[test]
fn dropins_then_user_then_explicit_project() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("cfg/rafa");
    write(&cfg.join("mcp.d/10-nix.yaml"), "version: 1\nservers:\n  slack: { prefix: slack, timeout: 10 }\n");
    write(&cfg.join("mcp.yaml"), "servers:\n  scratch: { transport: { unix: \"${HOME}/s.sock\" } }\nprofiles:\n  p: { servers: { slack: {} } }\n");
    let project = d.path().join("proj");
    write(&project.join(".rafa/mcp.yaml"), "profiles:\n  q: { servers: { scratch: {} } }\n");
    let c = load(&env(d.path()), None).unwrap();
    assert_eq!(c.servers["slack"].timeout_secs, Some(10));
    assert_eq!(c.servers["scratch"].transport, Some(mcp::config::Transport::Unix(format!("{}/s.sock", d.path().display()))));
    assert!(!c.profiles.contains_key("q"), "the project file loads only with --project");
    let c = load(&env(d.path()), Some(&project)).unwrap();
    assert!(c.profiles.contains_key("q"));
}

#[test]
fn a_duplicate_definition_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("cfg/rafa");
    write(&cfg.join("mcp.d/10-nix.yaml"), "servers:\n  slack: { prefix: a }\n");
    write(&cfg.join("mcp.yaml"), "servers:\n  slack: { prefix: b }\n");
    assert!(matches!(load(&env(d.path()), None), Err(LoadError::Config(rafa_config::ConfigError::Duplicate { .. }))));
}

#[test]
fn unreadable_and_invalid_files_are_errors() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("cfg/rafa");
    std::fs::create_dir_all(cfg.join("mcp.yaml")).unwrap();
    assert!(matches!(load(&env(d.path()), None), Err(LoadError::Read { .. })));
    std::fs::remove_dir(cfg.join("mcp.yaml")).unwrap();
    write(&cfg.join("mcp.yaml"), "servers:\n  s: { timeout: soon }\n");
    assert!(matches!(load(&env(d.path()), None), Err(LoadError::Mcp(_))));
}

#[test]
fn notify_sends_the_state_to_the_socket() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notify");
    let sock = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
    assert!(notify(Some(path.to_str().unwrap()), "READY=1").unwrap());
    let mut buf = [0u8; 64];
    let n = sock.recv(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"READY=1");
    assert!(!notify(None, "READY=1").unwrap());
}

#[test]
fn notify_supports_abstract_sockets() {
    use std::os::linux::net::SocketAddrExt;
    let name = format!("mcp-test-{}", std::process::id());
    let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
    let sock = std::os::unix::net::UnixDatagram::bind_addr(&addr).unwrap();
    assert!(notify(Some(&format!("@{name}")), "STOPPING=1").unwrap());
    let mut buf = [0u8; 64];
    let n = sock.recv(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"STOPPING=1");
}

#[test]
fn activation_needs_our_pid_and_at_least_one_fd() {
    assert_eq!(activated_fd(42, Some("42"), Some("1")), Some(3));
    assert_eq!(activated_fd(42, Some("43"), Some("1")), None);
    assert_eq!(activated_fd(42, Some("42"), Some("0")), None);
    assert_eq!(activated_fd(42, None, Some("1")), None);
    assert_eq!(activated_fd(42, Some("42"), Some("x")), None);
}

#[tokio::test]
async fn dir_watch_wakes_on_create_and_delete() {
    let d = tempfile::tempdir().unwrap();
    let mut w = DirWatch::new(d.path()).unwrap();
    let path = d.path().join("slack.sock");
    let _l = std::os::unix::net::UnixListener::bind(&path).unwrap();
    tokio::time::timeout(Duration::from_secs(2), w.changed()).await.expect("create").unwrap();
    std::fs::remove_file(&path).unwrap();
    tokio::time::timeout(Duration::from_secs(2), w.changed()).await.expect("delete").unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(200), w.changed()).await.is_err(), "woke with no change");
}

#[test]
fn an_unreadable_dropin_directory_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    write(&d.path().join("cfg/rafa/mcp.d"), "not a directory");
    assert!(matches!(load(&env(d.path()), None), Err(LoadError::Read { ref path, .. }) if path.ends_with("mcp.d")));
}

#[test]
fn watching_a_missing_directory_is_an_error() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async { assert!(DirWatch::new(std::path::Path::new("/nonexistent/pool")).is_err()) });
}

#[test]
fn without_a_runtime_dir_only_the_unset_defaults_fail_and_only_when_used() {
    let d = tempfile::tempdir().unwrap();
    let mut e = env(d.path());
    e.remove("XDG_RUNTIME_DIR");
    write(&d.path().join("cfg/rafa/mcp.yaml"), "proxy: { socket: /run/explicit.sock }\n");
    let c = load(&e, None).expect("loading must not need XDG_RUNTIME_DIR");
    assert_eq!(c.proxy.socket_path(&e).unwrap(), std::path::PathBuf::from("/run/explicit.sock"));
    assert!(c.proxy.pool_path(&e).unwrap_err().contains("XDG_RUNTIME_DIR"));
}
