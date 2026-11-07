mod common;

use std::time::Duration;

use common::{destructive, serve_mock, text_of, tool};
use mcp::{config::from_value, proxy::{Proxy, META_ORIGINAL, META_SERVER}};
use rafa_config::Layer;
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, RoleClient, ServiceExt};
use tokio::net::{UnixListener, UnixStream};

const CONFIG: &str = r#"
servers:
  slack:
    prefix: chat
    exclude: ["secret_*", { annotations: { destructiveHint: true } }]
    rename: { conversations_history: history }
    describe: { search: "Search the workspace." }
  slow:
    timeout: 1s
"#;

struct Rig {
    _dir: tempfile::TempDir,
    gtasks_cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pool: std::path::PathBuf,
    socket: std::path::PathBuf,
    proxy: Proxy,
}

async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let pool = dir.path().join("servers");
    std::fs::create_dir(&pool).unwrap();
    serve_mock(&pool.join("slack.sock"), vec![tool("search"), tool("conversations_history"), tool("secret_tokens"), destructive("delete_all")]);
    let gtasks = serve_mock(&pool.join("gtasks.sock"), vec![tool("list_tasks"), tool("progress"), tool("hang"), tool("nap"), tool("fail")]);
    let config = from_value(&Layer::parse("t", CONFIG).unwrap().value).unwrap();
    let proxy = Proxy::new(config);
    assert_eq!(proxy.sync_pool(&pool).await, Vec::<String>::new());
    let socket = dir.path().join("proxy.sock");
    tokio::spawn(proxy.clone().run(UnixListener::bind(&socket).unwrap()));
    Rig { _dir: dir, gtasks_cancelled: gtasks.cancelled.clone(), pool, socket, proxy }
}

async fn agent(socket: &std::path::Path) -> rmcp::service::RunningService<RoleClient, ()> {
    ().serve(UnixStream::connect(socket).await.unwrap()).await.unwrap()
}

fn call(name: &str) -> CallToolRequestParams {
    CallToolRequestParams::new(name.to_string())
}

#[tokio::test]
async fn lists_every_server_through_its_proxy_side_rules() {
    let rig = rig().await;
    let client = agent(&rig.socket).await;
    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["chat-history", "chat-search", "gtasks-fail", "gtasks-hang", "gtasks-list_tasks", "gtasks-nap", "gtasks-progress"]);
    let search = tools.iter().find(|t| t.name == "chat-search").unwrap();
    assert_eq!(search.description.as_deref(), Some("Search the workspace."));
    let meta = &search.meta.as_ref().unwrap().0;
    assert_eq!(meta[META_SERVER], "slack");
    assert_eq!(meta[META_ORIGINAL], "search");
}

#[tokio::test]
async fn calls_reach_the_original_tool_with_their_arguments() {
    let rig = rig().await;
    let client = agent(&rig.socket).await;
    let mut params = call("chat-history");
    params.arguments = Some(serde_json::json!({ "channel": "C1" }).as_object().unwrap().clone());
    let CallToolResponse::Complete(result) = client.call_tool_once(params).await.unwrap() else { panic!("not complete") };
    assert_eq!(text_of(&result), r#"called conversations_history with {"channel":"C1"}"#);
}

#[tokio::test]
async fn hidden_renamed_and_unknown_names_are_not_callable() {
    let rig = rig().await;
    let client = agent(&rig.socket).await;
    for name in ["chat-secret_tokens", "chat-delete_all", "chat-conversations_history", "nope"] {
        assert!(client.call_tool_once(call(name)).await.is_err(), "{name} was callable");
    }
}

#[tokio::test]
async fn a_server_that_does_not_answer_in_time_returns_a_tool_error() {
    let rig = rig().await;
    serve_mock(&rig.pool.join("slow.sock"), vec![tool("slow")]);
    rig.proxy.sync_pool(&rig.pool).await;
    let client = agent(&rig.socket).await;
    let started = std::time::Instant::now();
    let CallToolResponse::Complete(result) = client.call_tool_once(call("slow-slow")).await.unwrap() else { panic!() };
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(result.is_error, Some(true));
    assert!(text_of(&result).contains("no answer within 1s"), "{}", text_of(&result));
}

#[tokio::test]
async fn servers_appear_and_leave_with_their_sockets() {
    let rig = rig().await;
    serve_mock(&rig.pool.join("web.sock"), vec![tool("browse")]);
    rig.proxy.sync_pool(&rig.pool).await;
    assert!(rig.proxy.backend_names().await.contains(&"web".to_string()));
    std::fs::remove_file(rig.pool.join("web.sock")).unwrap();
    rig.proxy.sync_pool(&rig.pool).await;
    assert!(!rig.proxy.backend_names().await.contains(&"web".to_string()));
    let client = agent(&rig.socket).await;
    let names: Vec<_> = client.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
    assert!(!names.iter().any(|n| n.starts_with("web-")));
}

#[tokio::test]
async fn each_agent_gets_its_own_session() {
    let rig = rig().await;
    let a = agent(&rig.socket).await;
    let b = agent(&rig.socket).await;
    a.cancel().await.unwrap();
    let CallToolResponse::Complete(result) = b.call_tool_once(call("gtasks-list_tasks")).await.unwrap() else { panic!() };
    assert!(text_of(&result).contains("list_tasks"));
}

#[tokio::test]
async fn progress_from_the_server_reaches_the_agent() {
    let rig = rig().await;
    let recorder = common::Recorder::default();
    let client = recorder.clone().serve(UnixStream::connect(&rig.socket).await.unwrap()).await.unwrap();
    client.call_tool_once(call("gtasks-progress")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*recorder.progress.lock().unwrap(), vec![1.0, 2.0]);
}

#[tokio::test]
async fn an_agent_cancel_reaches_the_server() {
    let rig = rig().await;
    let client = agent(&rig.socket).await;
    let _ = tokio::time::timeout(Duration::from_millis(200), client.call_tool_once(call("gtasks-hang"))).await;
    for _ in 0..40 {
        if rig.gtasks_cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server never saw the cancellation");
}

#[tokio::test]
async fn shutdown_drains_in_flight_calls_then_refuses_connections() {
    let rig = rig().await;
    let client = agent(&rig.socket).await;
    let call = tokio::spawn(async move { client.call_tool_once(call("gtasks-nap")).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    rig.proxy.shutdown(Duration::from_secs(5)).await;
    let CallToolResponse::Complete(result) = call.await.unwrap().unwrap() else { panic!() };
    assert!(text_of(&result).contains("called nap"));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(UnixStream::connect(&rig.socket).await.is_err(), "still accepting after shutdown");
}

#[tokio::test]
async fn shutdown_gives_up_on_calls_that_outlive_the_grace() {
    let rig = rig().await;
    let client = agent(&rig.socket).await;
    let call = tokio::spawn(async move { client.call_tool_once(call("gtasks-hang")).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let started = std::time::Instant::now();
    rig.proxy.shutdown(Duration::from_millis(200)).await;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(tokio::time::timeout(Duration::from_secs(2), call).await.unwrap().unwrap().is_err());
}

#[tokio::test]
async fn a_restarted_server_is_reconnected_on_the_next_call() {
    let rig = rig().await;
    let web = serve_mock(&rig.pool.join("web.sock"), vec![tool("browse")]);
    rig.proxy.sync_pool(&rig.pool).await;
    let client = agent(&rig.socket).await;
    assert!(client.call_tool_once(call("web-browse")).await.is_ok());
    web.kill();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _web2 = serve_mock(&rig.pool.join("web.sock"), vec![tool("browse")]);
    let CallToolResponse::Complete(r) = client.call_tool_once(call("web-browse")).await.unwrap() else { panic!() };
    assert!(text_of(&r).contains("called browse"));
}

#[tokio::test]
async fn a_server_tool_change_reaches_the_catalog() {
    let rig = rig().await;
    let web = serve_mock(&rig.pool.join("web.sock"), vec![tool("browse")]);
    rig.proxy.sync_pool(&rig.pool).await;
    let client = agent(&rig.socket).await;
    web.set_tools(vec![tool("browse"), tool("screenshot")]).await;
    for _ in 0..40 {
        let names: Vec<_> = client.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
        if names.contains(&"web-screenshot".to_string()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the new tool never appeared");
}

#[tokio::test]
async fn configured_unix_transports_connect_outside_the_pool() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("remote.sock");
    let _m = serve_mock(&elsewhere, vec![tool("ping")]);
    let yaml = format!("servers:\n  remote: {{ transport: {{ unix: \"{}\" }} }}\n", elsewhere.display());
    let proxy = Proxy::new(from_value(&Layer::parse("t", &yaml).unwrap().value).unwrap());
    assert_eq!(proxy.connect_configured().await, Vec::<String>::new());
    // an empty pool must not drop a configured server
    std::fs::create_dir(dir.path().join("pool")).unwrap();
    proxy.sync_pool(&dir.path().join("pool")).await;
    assert_eq!(proxy.backend_names().await, vec!["remote".to_string()]);
}

#[tokio::test]
async fn a_server_error_reaches_the_agent_unchanged() {
    let rig = rig().await;
    let client = agent(&rig.socket).await;
    let err = client.call_tool_once(call("gtasks-fail")).await.unwrap_err().to_string();
    assert!(err.contains("the mock refuses"), "{err}");
}

#[tokio::test]
async fn calls_during_the_drain_are_refused() {
    let rig = rig().await;
    let (a, b) = (agent(&rig.socket).await, agent(&rig.socket).await);
    let running = tokio::spawn(async move { a.call_tool_once(call("gtasks-nap")).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let proxy = rig.proxy.clone();
    let draining = tokio::spawn(async move { proxy.shutdown(Duration::from_secs(5)).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let err = b.call_tool_once(call("gtasks-list_tasks")).await.unwrap_err().to_string();
    assert!(err.contains("shutting down"), "{err}");
    assert!(running.await.unwrap().is_ok());
    draining.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn configured_stdio_transports_run_a_child_with_its_env() {
    // the child is `mcp client`, itself a stdio MCP server, talking to the rig's proxy
    let rig = rig().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("cfg/rafa")).unwrap();
    std::fs::write(dir.path().join("cfg/rafa/mcp.yaml"), format!("proxy: {{ socket: \"{}\" }}\n", rig.socket.display())).unwrap();
    let mut env = format!("XDG_CONFIG_HOME: \"{}\", HOME: \"{}\"", dir.path().join("cfg").display(), dir.path().display());
    if let Ok(cov) = std::env::var("LLVM_PROFILE_FILE") {
        env.push_str(&format!(", LLVM_PROFILE_FILE: \"{cov}\""));
    }
    let yaml = format!(
        "servers:\n  nested: {{ transport: {{ stdio: {{ command: \"{}\", args: [client, --server, gtasks, --include, list_tasks], env: {{ {env} }} }} }} }}\n",
        env!("CARGO_BIN_EXE_mcp")
    );
    let outer = Proxy::new(from_value(&Layer::parse("t", &yaml).unwrap().value).unwrap());
    assert_eq!(outer.connect_configured().await, Vec::<String>::new());
    let (catalog, tools) = outer.catalog().await.unwrap();
    assert_eq!(tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>(), vec!["nested-gtasks-list_tasks"]);
    assert_eq!(catalog.resolve("nested-gtasks-list_tasks"), Some(("nested", "gtasks-list_tasks")));
    assert!(Proxy::new(from_value(&Layer::parse("t", "servers:\n  x: { transport: { stdio: { command: /nonexistent/x } } }\n").unwrap().value).unwrap())
        .connect_configured()
        .await[0]
        .contains("spawn /nonexistent/x"));
}

#[tokio::test]
async fn a_name_collision_is_refused_and_names_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    let pool = dir.path().join("pool");
    std::fs::create_dir(&pool).unwrap();
    let _a = serve_mock(&pool.join("a.sock"), vec![tool("x")]);
    let _b = serve_mock(&pool.join("b.sock"), vec![tool("x")]);
    let config = from_value(&Layer::parse("t", "servers:\n  a: { prefix: \"\" }\n  b: { prefix: \"\" }\n").unwrap().value).unwrap();
    let proxy = Proxy::new(config);
    proxy.sync_pool(&pool).await;
    let socket = dir.path().join("p.sock");
    tokio::spawn(proxy.clone().run(UnixListener::bind(&socket).unwrap()));
    let client = agent(&socket).await;
    let err = client.list_all_tools().await.unwrap_err().to_string();
    assert!(err.contains("\"x\"") && err.contains('a') && err.contains('b'), "{err}");
    assert!(client.call_tool_once(call("x")).await.is_err());
}
