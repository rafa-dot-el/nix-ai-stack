//! `mcp sidecar`: serve a stdio-only MCP server on an activated unix socket, one child
//! process per connection, bytes copied both ways (no MCP parsing, so nothing is lost).
use std::{process::Stdio, time::Duration};

use tokio::{
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
    process::Command,
};
use tokio_util::sync::CancellationToken;

/// How long a child may take to exit on its own after its connection closed.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Accept until `stop`; each connection gets its own `argv` child, killed when it closes.
pub async fn run(listener: UnixListener, argv: Vec<String>, stop: CancellationToken) -> std::io::Result<()> {
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => accepted?.0,
            _ = stop.cancelled() => return Ok(()),
        };
        let (argv, stop) = (argv.clone(), stop.clone());
        tokio::spawn(async move {
            if let Err(e) = connection(stream, &argv, stop).await {
                eprintln!("[-] mcp sidecar: {}: {e}", argv[0]);
            }
        });
    }
}

async fn connection(stream: UnixStream, argv: &[String], stop: CancellationToken) -> std::io::Result<()> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let (mut child_in, mut child_out) = (child.stdin.take().expect("piped"), child.stdout.take().expect("piped"));
    let (mut read, mut write) = stream.into_split();
    let up = async {
        let _ = tokio::io::copy(&mut read, &mut child_in).await;
        drop(child_in);
    };
    let down = async {
        let _ = tokio::io::copy(&mut child_out, &mut write).await;
        let _ = write.shutdown().await;
    };
    tokio::pin!(up, down);
    tokio::select! {
        _ = &mut down => {}
        _ = &mut up => {
            let _ = tokio::time::timeout(EXIT_GRACE, child.wait()).await;
        }
        _ = stop.cancelled() => {}
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
    Ok(())
}
