//! `mcp proxy`: one MCP server for every agent, in front of every MCP server (ADR-0004).
//!
//! Each backend is an rmcp client over a unix socket (normally a socket-activated unit in
//! the pool directory). Each agent connection is its own rmcp server session, so requests,
//! cancellation and errors never cross between clients. Tools are filtered with the
//! servers' proxy-side rules (mcp-filter) and carry `_meta` naming their server and
//! original name, which `mcp client` uses to apply a profile on original names.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use mcp_filter::{Catalog, FilterError, Server};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, ErrorCode, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerConfig, Tool,
    },
    service::{Peer, RequestContext, RunningService},
    ErrorData as McpError, RoleClient, RoleServer, ServerHandler, ServiceExt,
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Mutex, Notify, RwLock},
};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{prefix_of, McpConfig, Transport},
    relay::{forward_call, Relay},
};

/// `_meta` keys on every tool the proxy lists.
pub const META_SERVER: &str = "rafa/server";
pub const META_ORIGINAL: &str = "rafa/original";
/// Per-call timeout when a server sets none.
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// mcp-filter's view of an rmcp tool.
pub fn filter_tool(tool: &Tool) -> mcp_filter::Tool {
    let a = tool.annotations.as_ref();
    mcp_filter::Tool {
        name: tool.name.to_string(),
        description: tool.description.as_ref().map(|d| d.to_string()),
        annotations: mcp_filter::Annotations {
            read_only: a.and_then(|a| a.read_only_hint),
            destructive: a.and_then(|a| a.destructive_hint),
            idempotent: a.and_then(|a| a.idempotent_hint),
            open_world: a.and_then(|a| a.open_world_hint),
        },
    }
}

/// How a backend is reached; kept so a dead connection can be reopened.
#[derive(Clone, Debug)]
enum Endpoint {
    Socket(PathBuf),
    Stdio { command: String, args: Vec<String>, env: BTreeMap<String, String> },
}

struct Backend {
    service: RunningService<RoleClient, Relay>,
    /// Shared by every connection to this backend, so progress routes and the
    /// list-changed watcher survive a reconnect.
    relay: Relay,
    endpoint: Endpoint,
    /// From the pool directory (dropped when its socket goes) or from the config.
    pooled: bool,
    tools: Vec<Tool>,
    stop: CancellationToken,
}

async fn open(endpoint: &Endpoint, relay: &Relay) -> Result<RunningService<RoleClient, Relay>, String> {
    match endpoint {
        Endpoint::Socket(path) => {
            let stream = UnixStream::connect(path).await.map_err(|e| format!("connect {}: {e}", path.display()))?;
            relay.clone().serve(stream).await.map_err(|e| format!("initialize: {e}"))
        }
        Endpoint::Stdio { command, args, env } => {
            let mut cmd = tokio::process::Command::new(command);
            cmd.args(args).envs(env).kill_on_drop(true);
            let child = rmcp::transport::TokioChildProcess::new(cmd).map_err(|e| format!("spawn {command}: {e}"))?;
            relay.clone().serve(child).await.map_err(|e| format!("initialize: {e}"))
        }
    }
}

fn is_dead(service: &RunningService<RoleClient, Relay>) -> bool {
    service.is_closed() || service.peer().is_transport_closed()
}

struct Inner {
    config: McpConfig,
    backends: RwLock<BTreeMap<String, Backend>>,
    sessions: Mutex<Vec<Peer<RoleServer>>>,
    /// Cancelled first on shutdown: no new connections or calls.
    accepting: CancellationToken,
    /// Cancelled after the drain: every session closes.
    closing: CancellationToken,
    inflight: AtomicUsize,
    idle: Notify,
}

/// Counts one in-flight call for the drain.
struct InFlight<'a>(&'a Inner);

impl<'a> InFlight<'a> {
    fn new(inner: &'a Inner) -> Self {
        inner.inflight.fetch_add(1, Ordering::SeqCst);
        InFlight(inner)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if self.0.inflight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

#[derive(Clone)]
pub struct Proxy {
    inner: Arc<Inner>,
}

impl Proxy {
    pub fn new(config: McpConfig) -> Proxy {
        Proxy {
            inner: Arc::new(Inner {
                config,
                backends: RwLock::new(BTreeMap::new()),
                sessions: Mutex::new(Vec::new()),
                accepting: CancellationToken::new(),
                closing: CancellationToken::new(),
                inflight: AtomicUsize::new(0),
                idle: Notify::new(),
            }),
        }
    }

    /// Connect to a server's unix socket and register it under `name`.
    pub async fn connect(&self, name: &str, socket: &Path) -> Result<(), String> {
        self.add(name, Endpoint::Socket(socket.to_path_buf()), true).await
    }

    /// Connect every server with a `transport:` in the config; returns the failures.
    pub async fn connect_configured(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (name, server) in &self.inner.config.servers {
            let endpoint = match &server.transport {
                Some(Transport::Unix(path)) => Endpoint::Socket(PathBuf::from(path)),
                Some(Transport::Stdio { command, args, env }) => {
                    Endpoint::Stdio { command: command.clone(), args: args.clone(), env: env.clone() }
                }
                None => continue,
            };
            if let Err(e) = self.add(name, endpoint, false).await {
                errors.push(e);
            }
        }
        errors
    }

    async fn add(&self, name: &str, endpoint: Endpoint, pooled: bool) -> Result<(), String> {
        let relay = Relay::default();
        let service = open(&endpoint, &relay).await.map_err(|e| format!("{name}: {e}"))?;
        let tools = service.list_all_tools().await.map_err(|e| format!("{name}: tools/list: {e}"))?;
        let stop = CancellationToken::new();
        self.watch_tools(name.to_string(), relay.clone(), stop.clone());
        let old = self
            .inner
            .backends
            .write()
            .await
            .insert(name.to_string(), Backend { service, relay, endpoint, pooled, tools, stop });
        if let Some(old) = old {
            old.stop.cancel();
            let _ = old.service.cancel().await;
        }
        self.notify_changed().await;
        Ok(())
    }

    /// Re-list a backend's tools whenever it says they changed.
    fn watch_tools(&self, name: String, relay: Relay, stop: CancellationToken) {
        let proxy = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = relay.tools_changed.notified() => {}
                    _ = stop.cancelled() => return,
                }
                let peer = match proxy.inner.backends.read().await.get(&name) {
                    Some(b) => b.service.peer().clone(),
                    None => return,
                };
                match peer.list_all_tools().await {
                    Ok(tools) => {
                        if let Some(b) = proxy.inner.backends.write().await.get_mut(&name) {
                            b.tools = tools;
                        }
                        proxy.notify_changed().await;
                    }
                    Err(e) => eprintln!("[-] mcp proxy: {name}: tools/list after list_changed: {e}"),
                }
            }
        });
    }

    /// The backend's peer, reopening the connection first if it died (a restarted unit).
    async fn live(&self, name: &str) -> Result<(Peer<RoleClient>, Relay), String> {
        let (endpoint, relay) = {
            let backends = self.inner.backends.read().await;
            let b = backends.get(name).ok_or_else(|| format!("{name}: not connected"))?;
            if !is_dead(&b.service) {
                return Ok((b.service.peer().clone(), b.relay.clone()));
            }
            (b.endpoint.clone(), b.relay.clone())
        };
        let service = open(&endpoint, &relay).await.map_err(|e| format!("{name}: reconnect: {e}"))?;
        let peer = service.peer().clone();
        if let Some(b) = self.inner.backends.write().await.get_mut(name) {
            let old = std::mem::replace(&mut b.service, service);
            tokio::spawn(async move {
                let _ = old.cancel().await;
            });
        }
        Ok((peer, relay))
    }

    pub async fn remove(&self, name: &str) {
        let removed = self.inner.backends.write().await.remove(name);
        if let Some(backend) = removed {
            backend.stop.cancel();
            let _ = backend.service.cancel().await;
            self.notify_changed().await;
        }
    }

    pub async fn backend_names(&self) -> Vec<String> {
        self.inner.backends.read().await.keys().cloned().collect()
    }

    async fn notify_changed(&self) {
        let mut sessions = self.inner.sessions.lock().await;
        let mut alive = Vec::with_capacity(sessions.len());
        for peer in sessions.drain(..) {
            if peer.notify_tool_list_changed().await.is_ok() {
                alive.push(peer);
            }
        }
        *sessions = alive;
    }

    /// The filtered catalog over every connected backend, and each exposed tool's definition.
    pub async fn catalog(&self) -> Result<(Catalog, Vec<Tool>), FilterError> {
        let backends = self.inner.backends.read().await;
        let config = &self.inner.config;
        let empty = mcp_filter::Rules::default();
        let filter_tools: BTreeMap<&str, Vec<mcp_filter::Tool>> =
            backends.iter().map(|(name, b)| (name.as_str(), b.tools.iter().map(filter_tool).collect())).collect();
        let servers: Vec<Server<'_>> = backends
            .keys()
            .map(|name| {
                let server = config.servers.get(name);
                Server {
                    name,
                    prefix: prefix_of(name, server),
                    rules: server.map_or(&empty, |s| &s.rules),
                    tools: &filter_tools[name.as_str()],
                }
            })
            .collect();
        let catalog = Catalog::build(&servers, None, &config.proxy.separator)?;
        let tools = catalog
            .tools()
            .iter()
            .map(|exposed| {
                let original = backends[&exposed.server].tools.iter().find(|t| t.name == exposed.original).expect("listed tool");
                let mut tool = original.clone();
                tool.name = exposed.name.clone().into();
                tool.description = exposed.description.clone().map(Into::into);
                let mut meta = tool.meta.take().unwrap_or_default();
                meta.0.insert(META_SERVER.into(), exposed.server.clone().into());
                meta.0.insert(META_ORIGINAL.into(), exposed.original.clone().into());
                tool.meta = Some(meta);
                tool
            })
            .collect();
        Ok((catalog, tools))
    }

    /// Serve one agent connection until it closes.
    pub async fn serve_connection<S>(&self, stream: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        match self.clone().serve(stream).await {
            Ok(running) => {
                self.inner.sessions.lock().await.push(running.peer().clone());
                let closing = self.inner.closing.clone();
                let ct = running.cancellation_token();
                tokio::select! {
                    _ = running.waiting() => {}
                    _ = closing.cancelled() => ct.cancel(),
                }
            }
            Err(e) => eprintln!("[-] mcp proxy: session failed to initialise: {e}"),
        }
    }

    /// Stop accepting, let in-flight calls finish within `grace`, then close every session.
    pub async fn shutdown(&self, grace: Duration) {
        self.inner.accepting.cancel();
        let drained = async {
            loop {
                let idle = self.inner.idle.notified();
                if self.inner.inflight.load(Ordering::SeqCst) == 0 {
                    return;
                }
                idle.await;
            }
        };
        if tokio::time::timeout(grace, drained).await.is_err() {
            eprintln!("[-] mcp proxy: {} call(s) still running after the grace period", self.inner.inflight.load(Ordering::SeqCst));
        }
        self.inner.closing.cancel();
        let backends = std::mem::take(&mut *self.inner.backends.write().await);
        for (_, backend) in backends {
            let _ = backend.service.cancel().await;
        }
    }

    /// Accept agent connections until [`Proxy::shutdown`]; the listener closes on return.
    pub async fn run(self, listener: UnixListener) -> std::io::Result<()> {
        loop {
            let stream = tokio::select! {
                accepted = listener.accept() => accepted?.0,
                _ = self.inner.accepting.cancelled() => return Ok(()),
            };
            let proxy = self.clone();
            tokio::spawn(async move { proxy.serve_connection(stream).await });
        }
    }

    /// Bring the backends in line with the sockets in `pool`: connect new ones, drop gone ones.
    pub async fn sync_pool(&self, pool: &Path) -> Vec<String> {
        let mut errors = Vec::new();
        let present: BTreeMap<String, PathBuf> = pool_sockets(pool);
        let pooled: Vec<String> =
            self.inner.backends.read().await.iter().filter(|(_, b)| b.pooled).map(|(n, _)| n.clone()).collect();
        for name in pooled.iter().filter(|n| !present.contains_key(*n)) {
            self.remove(name).await;
        }
        let connected = self.backend_names().await;
        for (name, socket) in &present {
            if !connected.contains(name) && !self.configured_elsewhere(name) {
                if let Err(e) = self.connect(name, socket).await {
                    errors.push(e);
                }
            }
        }
        errors
    }

    fn configured_elsewhere(&self, name: &str) -> bool {
        self.inner.config.servers.get(name).is_some_and(|s| s.transport.is_some())
    }
}

/// `<name>.sock` entries of the pool directory.
pub fn pool_sockets(pool: &Path) -> BTreeMap<String, PathBuf> {
    let Ok(entries) = std::fs::read_dir(pool) else { return BTreeMap::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".sock")?.to_string();
            Some((name, path))
        })
        .collect()
}

fn not_found(name: &str) -> McpError {
    McpError::new(ErrorCode::METHOD_NOT_FOUND, format!("no tool named {name:?}"), None)
}

impl ServerHandler for Proxy {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_tool_list_changed().build())
            .with_instructions("rafa mcp proxy: every configured MCP server behind one endpoint")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let (_, tools) = self.catalog().await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if self.inner.accepting.is_cancelled() {
            return Err(McpError::internal_error("mcp proxy is shutting down", None));
        }
        let _inflight = InFlight::new(&self.inner);
        let (catalog, _) = self.catalog().await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let (server, original) = catalog.resolve(&request.name).ok_or_else(|| not_found(&request.name))?;
        let (server, original) = (server.to_string(), original.to_string());
        let (peer, relay) = self.live(&server).await.map_err(|e| McpError::internal_error(e, None))?;
        let secs = self.inner.config.servers.get(&server).and_then(|s| s.timeout_secs).unwrap_or(DEFAULT_TIMEOUT_SECS);
        request.name = original.into();
        forward_call(&peer, &relay, request, &context, Duration::from_secs(secs), &server).await
    }
}
