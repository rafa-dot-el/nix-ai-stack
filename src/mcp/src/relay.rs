//! Forwarding one `tools/call` across a hop (agent -> client -> proxy -> server) with its
//! progress notifications and cancellation. rmcp gives every outbound request a fresh
//! progress token, so [`Relay`] keeps upstream token -> (downstream peer, token) routes.
use std::{collections::HashMap, sync::Arc, time::Duration};

use rmcp::{
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResponse, CallToolResult, CancelledNotificationParam,
        ClientConfig, ClientRequest, ContentBlock, ProgressNotificationParam, ProgressToken, ServerResult,
    },
    service::PeerRequestOptions,
    service::{NotificationContext, Peer, RequestContext},
    ClientHandler, ErrorData as McpError, RoleClient, RoleServer,
};
use tokio::sync::Notify;

type Route = (Peer<RoleServer>, ProgressToken);

/// The client-side handler of a hop: routes progress down, signals list changes.
#[derive(Clone, Default)]
pub struct Relay {
    routes: Arc<std::sync::Mutex<HashMap<ProgressToken, Route>>>,
    /// Woken on `notifications/tools/list_changed` from upstream.
    pub tools_changed: Arc<Notify>,
    /// Sent as this hop's `initialize`: the downstream agent's own, so the proxy sees who it is.
    pub info: Arc<std::sync::OnceLock<ClientConfig>>,
}

impl ClientHandler for Relay {
    fn get_info(&self) -> ClientConfig {
        self.info.get().cloned().unwrap_or_default()
    }

    async fn on_progress(&self, params: ProgressNotificationParam, _: NotificationContext<RoleClient>) {
        let route = self.routes.lock().expect("routes").get(&params.progress_token).cloned();
        if let Some((peer, token)) = route {
            let mut down = params;
            down.progress_token = token;
            let _ = peer.notify_progress(down).await;
        }
    }

    async fn on_tool_list_changed(&self, _: NotificationContext<RoleClient>) {
        self.tools_changed.notify_waiters();
    }
}

/// Send `params` upstream on behalf of the downstream request `ctx`: progress is relayed,
/// a downstream cancel or the timeout cancels upstream, and the timeout becomes a tool error.
pub async fn forward_call(
    upstream: &Peer<RoleClient>,
    relay: &Relay,
    params: CallToolRequestParams,
    ctx: &RequestContext<RoleServer>,
    timeout: Duration,
    label: &str,
) -> Result<CallToolResponse, McpError> {
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
    let handle = upstream
        .send_request_with_option(request, PeerRequestOptions::no_options())
        .await
        .map_err(|e| McpError::internal_error(format!("{label}: {e}"), None))?;
    let (id, upstream_token) = (handle.id.clone(), handle.progress_token.clone());
    let _route = ctx.meta.get_progress_token().map(|down| RouteGuard::new(relay, upstream_token, (ctx.peer.clone(), down)));
    let cancel = |reason: &str| {
        let p = CancelledNotificationParam::new(Some(id.clone()), Some(reason.to_string()));
        let upstream = upstream.clone();
        tokio::spawn(async move { upstream.notify_cancelled(p).await });
    };
    tokio::select! {
        response = handle.await_response() => match response {
            Ok(ServerResult::CallToolResult(r)) => Ok(CallToolResponse::Complete(r)),
            Ok(ServerResult::InputRequiredResult(r)) => Ok(CallToolResponse::InputRequired(r)),
            Ok(ServerResult::CreateTaskResult(r)) => Ok(CallToolResponse::Task(r)),
            Ok(_) => Err(McpError::internal_error(format!("{label}: unexpected response to tools/call"), None)),
            Err(rmcp::ServiceError::McpError(e)) => Err(e),
            Err(e) => Err(McpError::internal_error(format!("{label}: {e}"), None)),
        },
        _ = ctx.ct.cancelled() => {
            cancel("cancelled by the client");
            Err(McpError::internal_error(format!("{label}: cancelled"), None))
        }
        _ = tokio::time::sleep(timeout) => {
            cancel("timed out in mcp");
            let secs = timeout.as_secs();
            Ok(CallToolResult::error(vec![ContentBlock::text(format!("{label}: no answer within {secs}s"))]).into())
        }
    }
}

/// Removes a progress route when the call ends, however it ends.
struct RouteGuard<'a> {
    relay: &'a Relay,
    token: ProgressToken,
}

impl<'a> RouteGuard<'a> {
    fn new(relay: &'a Relay, token: ProgressToken, route: Route) -> Self {
        relay.routes.lock().expect("routes").insert(token.clone(), route);
        RouteGuard { relay, token }
    }
}

impl Drop for RouteGuard<'_> {
    fn drop(&mut self) {
        self.relay.routes.lock().expect("routes").remove(&self.token);
    }
}
