//! How every first-party MCP server listens (ADR-0005): `serve` (stdio), `serve --socket
//! PATH`, or `serve --systemd-socket` (the fd systemd passed, READY=1 sent). One rmcp
//! session per connection; SIGTERM/SIGINT stop accepting and end the sessions.
use std::{
    os::{
        fd::{FromRawFd, RawFd},
        unix::fs::{DirBuilderExt, FileTypeExt},
    },
    path::{Path, PathBuf},
};

use rmcp::ServiceExt;
use tokio::{
    net::{UnixListener, UnixStream},
    signal::unix::{signal, SignalKind},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listen {
    Stdio,
    Socket(PathBuf),
    Systemd,
}

/// `[--socket PATH | --systemd-socket]` after `serve`; anything else is an error message.
pub fn parse_listen(args: &[String]) -> Result<Listen, String> {
    let mut listen = Listen::Stdio;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let next = match a.as_str() {
            "--socket" => Listen::Socket(it.next().filter(|v| !v.starts_with("--")).ok_or("--socket needs a PATH")?.into()),
            "--systemd-socket" => Listen::Systemd,
            other => return Err(format!("unknown flag {other:?} for serve (use --socket PATH or --systemd-socket)")),
        };
        if listen != Listen::Stdio {
            return Err("give one of --socket PATH or --systemd-socket".into());
        }
        listen = next;
    }
    Ok(listen)
}

/// `sd_notify(state)` when `socket` (the value of `$NOTIFY_SOCKET`) is set; `@` is abstract.
pub fn notify(socket: Option<&str>, state: &str) -> std::io::Result<bool> {
    use std::os::{linux::net::SocketAddrExt, unix::net::{SocketAddr, UnixDatagram}};
    let Some(socket) = socket.filter(|s| !s.is_empty()) else { return Ok(false) };
    let addr = match socket.strip_prefix('@') {
        Some(name) => SocketAddr::from_abstract_name(name.as_bytes())?,
        None => SocketAddr::from_pathname(socket)?,
    };
    UnixDatagram::unbound()?.send_to_addr(state.as_bytes(), &addr)?;
    Ok(true)
}

/// The first activated fd, when `$LISTEN_PID` names this process and `$LISTEN_FDS` >= 1.
pub fn activated_fd(pid: u32, listen_pid: Option<&str>, listen_fds: Option<&str>) -> Option<RawFd> {
    const SD_LISTEN_FDS_START: RawFd = 3;
    let ours = listen_pid?.parse::<u32>().ok()? == pid;
    let count = listen_fds?.parse::<u32>().ok()?;
    (ours && count >= 1).then_some(SD_LISTEN_FDS_START)
}

/// Bind `path` with mode 0600 (umask 0177 set before bind, so there is no chmod race), in a
/// 0700 parent; a stale socket is replaced, a live one or a non-socket is refused.
pub async fn bind_private(path: &Path) -> Result<UnixListener, String> {
    if let Some(parent) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if !meta.file_type().is_socket() {
            return Err(format!("{} exists and is not a socket", path.display()));
        }
        if UnixStream::connect(path).await.is_ok() {
            return Err(format!("{} is already being served", path.display()));
        }
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    // SAFETY: umask only changes this process's file-creation mask; restored right after.
    let old = unsafe { libc::umask(0o177) };
    let bound = UnixListener::bind(path);
    unsafe { libc::umask(old) };
    bound.map_err(|e| format!("bind {}: {e}", path.display()))
}

/// Resolves on SIGTERM or SIGINT.
pub async fn terminated() {
    let (mut term, mut int) = (signal(SignalKind::terminate()).expect("SIGTERM"), signal(SignalKind::interrupt()).expect("SIGINT"));
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

async fn accept<H, F>(listener: UnixListener, make: F, stop: CancellationToken)
where
    H: rmcp::ServerHandler,
    F: Fn() -> H + Send + Sync + 'static,
{
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((s, _)) => s,
                Err(e) => { eprintln!("[-] mcp-serve: accept: {e}"); continue }
            },
            _ = stop.cancelled() => return,
        };
        let (handler, stop) = (make(), stop.clone());
        tokio::spawn(async move {
            match handler.serve(stream).await {
                Ok(running) => {
                    let ct = running.cancellation_token();
                    tokio::select! {
                        _ = running.waiting() => {}
                        _ = stop.cancelled() => ct.cancel(),
                    }
                }
                Err(e) => eprintln!("[-] mcp-serve: session failed to initialise: {e}"),
            }
        });
    }
}

/// Serve `make()` handlers until stdin closes (stdio) or `stop` / a signal (sockets).
pub async fn serve<H, F>(listen: &Listen, make: F, stop: CancellationToken) -> Result<(), String>
where
    H: rmcp::ServerHandler,
    F: Fn() -> H + Send + Sync + 'static,
{
    let env = |k: &str| std::env::var(k).ok();
    let stopper = stop.clone();
    tokio::spawn(async move {
        terminated().await;
        stopper.cancel();
    });
    match listen {
        Listen::Stdio => {
            let running = make().serve((tokio::io::stdin(), tokio::io::stdout())).await.map_err(|e| format!("initialize: {e}"))?;
            let ct = running.cancellation_token();
            tokio::select! {
                _ = running.waiting() => {}
                _ = stop.cancelled() => ct.cancel(),
            }
            Ok(())
        }
        Listen::Socket(path) => {
            let listener = bind_private(path).await?;
            let _ = notify(env("NOTIFY_SOCKET").as_deref(), "READY=1");
            accept(listener, make, stop).await;
            let _ = std::fs::remove_file(path);
            Ok(())
        }
        Listen::Systemd => {
            let fd = activated_fd(std::process::id(), env("LISTEN_PID").as_deref(), env("LISTEN_FDS").as_deref())
                .ok_or("--systemd-socket needs a socket from systemd (LISTEN_PID/LISTEN_FDS not set for this process)")?;
            // SAFETY: systemd passed this fd to us (checked above) and nothing else owns it.
            let std_listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
            std_listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            let listener = UnixListener::from_std(std_listener).map_err(|e| e.to_string())?;
            let _ = notify(env("NOTIFY_SOCKET").as_deref(), "READY=1");
            accept(listener, make, stop.clone()).await;
            let _ = notify(env("NOTIFY_SOCKET").as_deref(), "STOPPING=1");
            Ok(())
        }
    }
}
