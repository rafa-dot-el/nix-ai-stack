//! `mcp client`: the stdio MCP server an agent runs, connected to `mcp proxy`.
//!
//! The pure half ([`view`], [`toonify`]) decides what the agent sees from the proxy's
//! tools and a profile; the effectful half ([`Client`]) holds the proxy connection.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::{NotificationContext, Peer, RequestContext, RunningService},
    ErrorData as McpError, RoleClient, RoleServer, ServerHandler, ServiceExt,
};
use tokio::{net::UnixStream, sync::Mutex};

use crate::{
    config::Profile,
    proxy::{filter_tool, DEFAULT_TIMEOUT_SECS, META_ORIGINAL, META_SERVER},
    relay::{forward_call, Relay},
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ViewError {
    #[error("tool {name:?} exposed twice: {first:?} and {second:?}")]
    Collision { name: String, first: String, second: String },
}

/// What the agent sees: the tools, and agent-facing name -> proxy name.
#[derive(Debug, Default)]
pub struct View {
    pub tools: Vec<Tool>,
    pub reverse: BTreeMap<String, String>,
}

/// Apply a profile to the proxy's tool list. Rules match each tool's original name under its
/// server (from `_meta`); a client rename is the final name verbatim; tools from servers the
/// profile does not list, or without `_meta`, are hidden.
pub fn view(proxy_tools: &[Tool], profile: &Profile) -> Result<View, ViewError> {
    let mut out = View::default();
    let mut tools = BTreeMap::new();
    for tool in proxy_tools {
        let meta = tool.meta.as_ref().map(|m| &m.0);
        let field = |key| meta.and_then(|m| m.get(key)).and_then(|v| v.as_str());
        let (Some(server), Some(original)) = (field(META_SERVER), field(META_ORIGINAL)) else { continue };
        let Some(rules) = profile.servers.get(server) else { continue };
        let mut probe = filter_tool(tool);
        probe.name = original.to_string();
        if !rules.visible(&probe) {
            continue;
        }
        let name = rules.rename.get(original).cloned().unwrap_or_else(|| tool.name.to_string());
        if let Some(first) = out.reverse.get(&name) {
            return Err(ViewError::Collision { name, first: first.clone(), second: tool.name.to_string() });
        }
        let mut exposed = tool.clone();
        exposed.name = name.clone().into();
        if let Some(text) = rules.describe.get(original) {
            exposed.description = Some(text.clone().into());
        }
        out.reverse.insert(name.clone(), tool.name.to_string());
        tools.insert(name, exposed);
    }
    out.tools = tools.into_values().collect();
    Ok(out)
}

/// TOON-encode every text block that is a JSON object or array; leave the rest as is.
pub fn toonify(mut result: CallToolResult) -> CallToolResult {
    result.content = result
        .content
        .into_iter()
        .map(|block| {
            let encoded = block.as_text().and_then(|t| match serde_json::from_str::<serde_json::Value>(&t.text) {
                Ok(v @ (serde_json::Value::Object(_) | serde_json::Value::Array(_))) => toon::encode_default(&v).ok(),
                _ => None,
            });
            encoded.map(ContentBlock::text).unwrap_or(block)
        })
        .collect();
    result
}

/// The agent-facing MCP server, connected to the proxy at `socket`.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    socket: PathBuf,
    profile: Profile,
    relay: Relay,
    upstream: Mutex<Option<RunningService<RoleClient, Relay>>>,
    view: Mutex<Option<Arc<View>>>,
}

fn unavailable(reason: String) -> McpError {
    McpError::internal_error(format!("mcp proxy unavailable: {reason}"), None)
}

impl Client {
    pub fn new(socket: PathBuf, profile: Profile) -> Client {
        Client {
            inner: Arc::new(Inner {
                socket,
                profile,
                relay: Relay::default(),
                upstream: Mutex::new(None),
                view: Mutex::new(None),
            }),
        }
    }

    /// Serve one agent over `io` (stdin/stdout in `mcp client`) until it closes.
    pub async fn serve_stdio_like<S>(self, io: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        match self.serve(io).await {
            Ok(running) => {
                let _ = running.waiting().await;
            }
            Err(e) => eprintln!("[-] mcp client: agent session failed to initialise: {e}"),
        }
    }

    /// The live proxy connection, (re)connecting and replaying `initialize` when it is gone.
    async fn upstream(&self) -> Result<Peer<RoleClient>, McpError> {
        let mut upstream = self.inner.upstream.lock().await;
        if let Some(running) = upstream.as_ref() {
            if !running.is_closed() && !running.peer().is_transport_closed() {
                return Ok(running.peer().clone());
            }
        }
        *self.inner.view.lock().await = None;
        let stream = UnixStream::connect(&self.inner.socket)
            .await
            .map_err(|e| unavailable(format!("{}: {e}", self.inner.socket.display())))?;
        let running = self.inner.relay.clone().serve(stream).await.map_err(|e| unavailable(e.to_string()))?;
        let peer = running.peer().clone();
        *upstream = Some(running);
        Ok(peer)
    }

    async fn current_view(&self) -> Result<(Peer<RoleClient>, Arc<View>), McpError> {
        let peer = self.upstream().await?;
        if let Some(v) = self.inner.view.lock().await.clone() {
            return Ok((peer, v));
        }
        let tools = peer.list_all_tools().await.map_err(|e| unavailable(e.to_string()))?;
        let v = Arc::new(view(&tools, &self.inner.profile).map_err(|e| McpError::internal_error(e.to_string(), None))?);
        *self.inner.view.lock().await = Some(v.clone());
        Ok((peer, v))
    }
}

impl ServerHandler for Client {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_tool_list_changed().build())
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        if let Some(info) = context.peer.peer_info() {
            let _ = self.inner.relay.info.set((*info).clone());
        }
        let (agent, inner) = (context.peer.clone(), self.inner.clone());
        tokio::spawn(async move {
            loop {
                inner.relay.tools_changed.notified().await;
                *inner.view.lock().await = None;
                if agent.notify_tool_list_changed().await.is_err() {
                    return;
                }
            }
        });
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let (_, v) = self.current_view().await?;
        Ok(ListToolsResult::with_all_items(v.tools.clone()))
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let (peer, v) = self.current_view().await?;
        let proxied = v.reverse.get(request.name.as_ref()).cloned().ok_or_else(|| {
            McpError::new(ErrorCode::METHOD_NOT_FOUND, format!("no tool named {:?}", request.name), None)
        })?;
        request.name = proxied.into();
        let secs = self.inner.profile.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        let response = forward_call(&peer, &self.inner.relay, request, &context, Duration::from_secs(secs), "mcp proxy").await?;
        Ok(match response {
            CallToolResponse::Complete(r) if self.inner.profile.toon => CallToolResponse::Complete(toonify(r)),
            other => other,
        })
    }
}
