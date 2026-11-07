//! A scriptable MCP server for tests: lists the tools it is given, echoes calls,
//! and `slow` sleeps longer than any test timeout.
#![allow(dead_code)]
use std::{path::Path, sync::{atomic::{AtomicBool, Ordering}, Arc}, time::Duration};

use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult, PaginatedRequestParams,
        ProgressNotificationParam, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
    },
    service::RequestContext,
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use tokio::{net::UnixListener, sync::Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Mock {
    pub tools: Arc<std::sync::RwLock<Vec<Tool>>>,
    /// Set when a `hang` call sees its cancellation.
    pub cancelled: Arc<AtomicBool>,
}

pub fn tool(name: &str) -> Tool {
    Tool::new(name.to_string(), format!("{name} from the mock"), Arc::new(serde_json::Map::new()))
}

pub fn destructive(name: &str) -> Tool {
    let mut t = tool(name);
    let mut a = ToolAnnotations::new();
    a.destructive_hint = Some(true);
    t.annotations = Some(a);
    t
}

impl ServerHandler for Mock {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tools.read().unwrap().clone()))
    }

    async fn call_tool(&self, request: CallToolRequestParams, ctx: RequestContext<RoleServer>) -> Result<CallToolResponse, McpError> {
        match request.name.as_ref() {
            "slow" => tokio::time::sleep(Duration::from_secs(30)).await,
            "nap" => tokio::time::sleep(Duration::from_millis(300)).await,
            "fail" => return Err(McpError::invalid_params("the mock refuses", None)),
            "json" => {
                return Ok(CallToolResult::success(vec![ContentBlock::text(r#"{"rows":[{"id":1,"t":"a"},{"id":2,"t":"b"}]}"#)]).into())
            }
            "hang" => {
                ctx.ct.cancelled().await;
                self.cancelled.store(true, Ordering::SeqCst);
                return Err(McpError::internal_error("cancelled", None));
            }
            "progress" => {
                let token = ctx.meta.get_progress_token().expect("caller sent a progress token");
                for step in 1..=2 {
                    let mut p = ProgressNotificationParam::new(token.clone(), f64::from(step));
                    p.total = Some(2.0);
                    ctx.peer.notify_progress(p).await.unwrap();
                }
            }
            _ => {}
        }
        let args = serde_json::Value::Object(request.arguments.unwrap_or_default());
        Ok(CallToolResult::success(vec![ContentBlock::text(format!("called {} with {args}", request.name))]).into())
    }
}

/// A running mock: its cancellation flag, and the handles to change or kill it.
pub struct MockHandle {
    pub cancelled: Arc<AtomicBool>,
    tools: Arc<std::sync::RwLock<Vec<Tool>>>,
    peers: Arc<Mutex<Vec<rmcp::service::Peer<RoleServer>>>>,
    kill: CancellationToken,
    path: std::path::PathBuf,
}

impl MockHandle {
    /// Replace the tool list and send tools/list_changed to every session.
    pub async fn set_tools(&self, tools: Vec<Tool>) {
        *self.tools.write().unwrap() = tools;
        for peer in self.peers.lock().await.iter() {
            let _ = peer.notify_tool_list_changed().await;
        }
    }

    /// Close every session and the listener, and remove the socket (a crashed server).
    pub fn kill(&self) {
        self.kill.cancel();
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Serve a mock on a unix socket at `path`, one rmcp session per connection.
pub fn serve_mock(path: &Path, tools: Vec<Tool>) -> MockHandle {
    let listener = UnixListener::bind(path).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let tools = Arc::new(std::sync::RwLock::new(tools));
    let peers = Arc::new(Mutex::new(Vec::new()));
    let kill = CancellationToken::new();
    let mock = Mock { tools: tools.clone(), cancelled: cancelled.clone() };
    let (k, ps) = (kill.clone(), peers.clone());
    tokio::spawn(async move {
        loop {
            let stream = tokio::select! {
                s = listener.accept() => s.unwrap().0,
                _ = k.cancelled() => return,
            };
            let (mock, k, ps) = (mock.clone(), k.clone(), ps.clone());
            tokio::spawn(async move {
                if let Ok(running) = mock.serve(stream).await {
                    ps.lock().await.push(running.peer().clone());
                    let ct = running.cancellation_token();
                    tokio::select! {
                        _ = running.waiting() => {}
                        _ = k.cancelled() => ct.cancel(),
                    }
                }
            });
        }
    });
    MockHandle { cancelled, tools, peers, kill, path: path.to_path_buf() }
}

/// An agent-side handler that records the progress it is sent.
#[derive(Clone, Default)]
pub struct Recorder {
    pub progress: Arc<std::sync::Mutex<Vec<f64>>>,
}

impl rmcp::ClientHandler for Recorder {
    async fn on_progress(&self, params: ProgressNotificationParam, _: rmcp::service::NotificationContext<rmcp::RoleClient>) {
        self.progress.lock().unwrap().push(params.progress);
    }
}

pub fn text_of(result: &CallToolResult) -> String {
    result.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect::<Vec<_>>().join("")
}
