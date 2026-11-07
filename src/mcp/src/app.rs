//! What each subcommand does, wired from the pure parts and the system boundary.
use std::{
    collections::BTreeMap,
    os::{fd::FromRawFd, unix::fs::DirBuilderExt},
    path::{Path, PathBuf},
    time::Duration,
};

use rmcp::ServiceExt;
use tokio::{
    net::{UnixListener, UnixStream},
};
use tokio_util::sync::CancellationToken;

use crate::{
    cli::Selection,
    client::Client,
    config::{inline_profile, McpConfig, Profile},
    doctor, proxy::Proxy, sidecar,
    system::{activated_fd, load, notify, DirWatch},
};

pub type Env = BTreeMap<String, String>;

/// How long SIGTERM waits for in-flight calls.
const DRAIN_GRACE: Duration = Duration::from_secs(10);
/// Retry delay after a pool sync left a server unconnected (e.g. bound but not listening yet).
const RETRY: Duration = Duration::from_secs(2);

fn notify_env(env: &Env, state: &str) {
    if let Err(e) = notify(env.get("NOTIFY_SOCKET").map(String::as_str), state) {
        eprintln!("[-] mcp: sd_notify {state}: {e}");
    }
}

fn private_dir(path: &Path) -> Result<(), String> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path).map_err(|e| format!("{}: {e}", path.display()))
}

use mcp_serve::{bind_private, terminated};

fn log_errors(errors: Vec<String>) -> bool {
    let failed = !errors.is_empty();
    for e in errors {
        eprintln!("[-] mcp proxy: {e}");
    }
    failed
}

/// `mcp proxy`: bind the proxy socket (0600), connect configured and pooled servers, watch
/// the pool, READY=1, and on SIGTERM/SIGINT drain then exit.
pub async fn proxy(env: &Env) -> Result<(), String> {
    let config = load(env, None).map_err(|e| e.to_string())?;
    let (socket, pool) = (config.proxy.socket_path(env)?, config.proxy.pool_path(env)?);
    private_dir(&pool)?;
    let listener = bind_private(&socket).await?;
    let proxy = Proxy::new(config);
    log_errors(proxy.connect_configured().await);
    let mut watch = DirWatch::new(&pool).map_err(|e| format!("watch {}: {e}", pool.display()))?;
    let mut retry = log_errors(proxy.sync_pool(&pool).await);
    let watcher = proxy.clone();
    tokio::spawn(async move {
        loop {
            if retry {
                let _ = tokio::time::timeout(RETRY, watch.changed()).await;
            } else if let Err(e) = watch.changed().await {
                eprintln!("[-] mcp proxy: pool watch: {e}");
                return;
            }
            retry = log_errors(watcher.sync_pool(&pool).await);
        }
    });
    let server = tokio::spawn(proxy.clone().run(listener));
    let names = proxy.backend_names().await;
    eprintln!("[+] mcp proxy: listening on {} with {} server(s): {}", socket.display(), names.len(), names.join(", "));
    notify_env(env, &format!("READY=1\nSTATUS=serving {} server(s)", names.len()));
    terminated().await;
    notify_env(env, "STOPPING=1");
    eprintln!("[*] mcp proxy: draining");
    proxy.shutdown(DRAIN_GRACE).await;
    let _ = server.await;
    let _ = std::fs::remove_file(&socket);
    Ok(())
}

fn profile(config: &McpConfig, selection: &Selection) -> Result<Profile, String> {
    match selection {
        Selection::Profile(name) => config.profiles.get(name).cloned().ok_or_else(|| {
            let known: Vec<&str> = config.profiles.keys().map(String::as_str).collect();
            format!("no profile {name:?} (known: {})", known.join(", "))
        }),
        Selection::Inline { servers, include, exclude, toon, timeout_secs } => {
            inline_profile(servers, include, exclude, *toon, *timeout_secs).map_err(|e| e.to_string())
        }
    }
}

fn loaded(env: &Env, project: bool) -> Result<McpConfig, String> {
    let dir = if project { Some(std::env::current_dir().map_err(|e| format!("current directory: {e}"))?) } else { None };
    load(env, dir.as_deref()).map_err(|e| e.to_string())
}

/// `mcp client`: an agent's stdio MCP server, connected to the proxy.
pub async fn client(env: &Env, selection: &Selection, project: bool) -> Result<(), String> {
    let config = loaded(env, project)?;
    let profile = profile(&config, selection)?;
    let io = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    Client::new(config.proxy.socket_path(env)?, profile).serve_stdio_like(io).await;
    Ok(())
}

/// `mcp sidecar`: a stdio server on the socket systemd passed in.
pub async fn sidecar(env: &Env, name: &str, argv: &[String]) -> Result<(), String> {
    let fd = activated_fd(std::process::id(), env.get("LISTEN_PID").map(String::as_str), env.get("LISTEN_FDS").map(String::as_str))
        .ok_or_else(|| format!("sidecar {name}: needs a socket from systemd (LISTEN_PID/LISTEN_FDS not set for this process)"))?;
    // SAFETY: systemd passed this fd to us (checked above) and nothing else owns it.
    let std_listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
    std_listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let listener = UnixListener::from_std(std_listener).map_err(|e| e.to_string())?;
    let stop = CancellationToken::new();
    let served = tokio::spawn(sidecar::run(listener, argv.to_vec(), stop.clone()));
    eprintln!("[+] mcp sidecar {name}: serving {}", argv[0]);
    notify_env(env, "READY=1");
    terminated().await;
    notify_env(env, "STOPPING=1");
    stop.cancel();
    served.await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

async fn proxy_tools(socket: &Path) -> Result<Vec<rmcp::model::Tool>, String> {
    let fetch = async {
        let stream = UnixStream::connect(socket).await.map_err(|e| format!("connect {}: {e}", socket.display()))?;
        let session = ().serve(stream).await.map_err(|e| format!("initialize: {e}"))?;
        let tools = session.list_all_tools().await.map_err(|e| format!("tools/list: {e}"));
        let _ = session.cancel().await;
        tools
    };
    tokio::time::timeout(Duration::from_secs(10), fetch).await.map_err(|_| "no answer within 10s".to_string())?
}

/// `mcp doctor`: print the report; `Ok(false)` when it has findings.
pub async fn doctor(env: &Env, profile: Option<&str>, json: bool) -> Result<bool, String> {
    let config = load(env, None).map_err(|e| format!("configuration: {e}"))?;
    let tools = proxy_tools(&config.proxy.socket_path(env)?).await;
    let report = doctor::report(&config, tools.as_deref().map_err(Clone::clone), profile)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&doctor::render_json(&report)).expect("json"));
    } else {
        print!("{}", doctor::render_text(&report));
    }
    Ok(report.ok())
}

/// `mcp config ...`: edit or show the user's layer.
pub fn config(env: &Env, command: &crate::cli::ConfigCommand) -> Result<(), String> {
    use crate::{cli::ConfigCommand, edit};
    let layers = crate::system::read_layers(env, None).map_err(|e| e.to_string())?;
    let path = layers.user_path.clone();
    let origin = path.display().to_string();
    let user = layers
        .user
        .clone()
        .unwrap_or_else(|| rafa_config::Layer { origin: origin.clone(), value: rafa_config::Value::Mapping(Default::default()) });
    let (value, done) = match command {
        ConfigCommand::Show { origins } => {
            print!("{}", edit::show(&layers.all(), *origins).map_err(|e| e.to_string())?);
            return Ok(());
        }
        ConfigCommand::AddMcp { name, spec } => (edit::add_server(&user, &layers.dropins, name, spec), format!("added server {name}")),
        ConfigCommand::AddProfile { name, spec } => (edit::add_profile(&user, &layers.dropins, name, spec), format!("added profile {name}")),
        ConfigCommand::Remove { kind, name } => (edit::remove(&user, &layers.dropins, *kind, name), format!("removed {name}")),
    };
    let value = value.map_err(|e| e.to_string())?;
    let text = serde_yaml_ng::to_string(&value).map_err(|e| e.to_string())?;
    crate::system::write_atomic(&path, &text).map_err(|e| format!("write {origin}: {e}"))?;
    eprintln!("[+] mcp config: {done} in {origin}");
    Ok(())
}

/// Every server's raw tool list for the TUI: pool sockets and configured unix servers are
/// asked directly (a pool socket activates its server); the proxy's own list is filtered.
pub async fn inventory(config: &McpConfig, pool: &Path) -> crate::tui::model::Inventory {
    use crate::config::Transport;
    let mut targets: BTreeMap<String, Result<PathBuf, String>> =
        crate::proxy::pool_sockets(pool).into_iter().map(|(n, p)| (n, Ok(p))).collect();
    for (name, server) in &config.servers {
        match &server.transport {
            Some(Transport::Unix(path)) => {
                targets.insert(name.clone(), Ok(PathBuf::from(path)));
            }
            Some(Transport::Stdio { command, .. }) => {
                targets.insert(name.clone(), Err(format!("stdio server ({command}): not started from here; see mcp doctor")));
            }
            None => {}
        }
    }
    let tasks: Vec<_> = targets
        .into_iter()
        .map(|(name, target)| {
            tokio::spawn(async move {
                let listed = match target {
                    Ok(path) => proxy_tools(&path).await,
                    Err(e) => Err(e),
                };
                (name, listed)
            })
        })
        .collect();
    let mut out = crate::tui::model::Inventory::new();
    for task in tasks {
        if let Ok((name, listed)) = task.await {
            out.insert(name, listed);
        }
    }
    out
}

/// `mcp config-tui`.
pub async fn config_tui(env: &Env) -> Result<(), String> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("mcp config-tui needs a terminal; use `mcp config ...` in scripts".into());
    }
    let layers = crate::system::read_layers(env, None).map_err(|e| e.to_string())?;
    let config = load(env, None).map_err(|e| e.to_string())?;
    let inventory = inventory(&config, &config.proxy.pool_path(env)?).await;
    let path = layers.user_path.clone();
    let user = layers.user.clone().unwrap_or_else(|| rafa_config::Layer {
        origin: path.display().to_string(),
        value: rafa_config::Value::Mapping(Default::default()),
    });
    let app = crate::tui::model::App::new(inventory, layers.dropins, user);
    tokio::task::spawn_blocking(move || {
        crate::tui::run(app, |value| {
            let text = serde_yaml_ng::to_string(value).map_err(|e| e.to_string())?;
            crate::system::write_atomic(&path, &text).map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
