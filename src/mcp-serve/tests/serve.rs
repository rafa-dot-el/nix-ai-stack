use std::{sync::Arc, time::Duration};

use mcp_serve::{activated_fd, notify, parse_listen, serve, Listen};
use rmcp::{
    model::{ServerCapabilities, ServerConfig},
    ServerHandler, ServiceExt,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Hello;

impl ServerHandler for Hello {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
}

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

#[test]
fn listen_flags() {
    assert_eq!(parse_listen(&[]).unwrap(), Listen::Stdio);
    assert_eq!(parse_listen(&args("--socket /run/a.sock")).unwrap(), Listen::Socket("/run/a.sock".into()));
    assert_eq!(parse_listen(&args("--systemd-socket")).unwrap(), Listen::Systemd);
    assert!(parse_listen(&args("--socket")).unwrap_err().contains("--socket"));
    assert!(parse_listen(&args("--socket a --systemd-socket")).is_err());
    assert!(parse_listen(&args("--port 3")).unwrap_err().contains("--port"));
}

#[test]
fn activation_needs_our_pid_and_an_fd() {
    assert_eq!(activated_fd(7, Some("7"), Some("2")), Some(3));
    assert_eq!(activated_fd(7, Some("8"), Some("1")), None);
    assert_eq!(activated_fd(7, Some("7"), Some("0")), None);
    assert_eq!(activated_fd(7, None, None), None);
}

#[test]
fn notify_reaches_the_socket() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("n");
    let sock = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
    assert!(notify(Some(path.to_str().unwrap()), "READY=1").unwrap());
    let mut buf = [0u8; 16];
    let n = sock.recv(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"READY=1");
    assert!(!notify(None, "READY=1").unwrap());
}

#[tokio::test]
async fn a_socket_serves_one_session_per_connection_until_stopped() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("s.sock");
    let stop = CancellationToken::new();
    let made = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = made.clone();
    let task = tokio::spawn({
        let (listen, stop) = (Listen::Socket(path.clone()), stop.clone());
        async move {
            serve(&listen, move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Hello
            }, stop)
            .await
        }
    });
    for _ in 0..50 {
        if path.exists() { break }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    for _ in 0..2 {
        let client = ().serve(tokio::net::UnixStream::connect(&path).await.unwrap()).await.unwrap();
        assert!(client.peer_info().is_some());
        client.cancel().await.unwrap();
    }
    assert_eq!(made.load(std::sync::atomic::Ordering::SeqCst), 2);
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
    assert!(!path.exists(), "the socket is removed on stop");
}

#[tokio::test]
async fn a_live_socket_is_not_stolen() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("s.sock");
    let _held = tokio::net::UnixListener::bind(&path).unwrap();
    let err = serve(&Listen::Socket(path), || Hello, CancellationToken::new()).await.unwrap_err();
    assert!(err.contains("already"), "{err}");
}

#[tokio::test]
async fn systemd_mode_without_activation_is_an_error() {
    let err = serve(&Listen::Systemd, || Hello, CancellationToken::new()).await.unwrap_err();
    assert!(err.contains("LISTEN_FDS"), "{err}");
}
