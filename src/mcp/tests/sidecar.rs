use std::time::Duration;

use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{UnixListener, UnixStream}};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn every_connection_gets_its_own_child_and_bytes_flow_both_ways() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("s.sock");
    let stop = CancellationToken::new();
    let task = tokio::spawn(mcp::sidecar::run(UnixListener::bind(&path).unwrap(), vec!["cat".into()], stop.clone()));
    for word in ["one", "two"] {
        let mut s = BufReader::new(UnixStream::connect(&path).await.unwrap());
        s.get_mut().write_all(format!("{word}\n").as_bytes()).await.unwrap();
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), s.read_line(&mut line)).await.unwrap().unwrap();
        assert_eq!(line, format!("{word}\n"));
    }
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn closing_the_connection_ends_the_child() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("s.sock");
    let marker = d.path().join("child.pid");
    let script = format!("echo $$ > {}; exec cat", marker.display());
    let stop = CancellationToken::new();
    tokio::spawn(mcp::sidecar::run(UnixListener::bind(&path).unwrap(), vec!["sh".into(), "-c".into(), script], stop.clone()));
    let s = UnixStream::connect(&path).await.unwrap();
    let mut pid = String::new();
    for _ in 0..40 {
        pid = std::fs::read_to_string(&marker).unwrap_or_default();
        if pid.ends_with('\n') { break }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let pid = pid.trim().to_string();
    assert!(!pid.is_empty());
    drop(s);
    for _ in 0..80 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() || std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default().contains(") Z ") {
            stop.cancel();
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("child {pid} outlived its connection");
}

#[tokio::test]
async fn a_command_that_cannot_start_closes_only_that_connection() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("s.sock");
    let stop = CancellationToken::new();
    let task = tokio::spawn(mcp::sidecar::run(UnixListener::bind(&path).unwrap(), vec!["/nonexistent/mcp-server".into()], stop.clone()));
    let mut s = UnixStream::connect(&path).await.unwrap();
    let mut buf = Vec::new();
    let n = tokio::time::timeout(Duration::from_secs(2), tokio::io::AsyncReadExt::read_to_end(&mut s, &mut buf)).await.unwrap().unwrap();
    assert_eq!(n, 0);
    assert!(!task.is_finished(), "the sidecar must keep accepting");
    stop.cancel();
}
