mod common;

use std::{sync::Arc, time::Duration};

use mcp::{client::{toonify, view, ViewError}, config::{inline_profile, Profile}, proxy::{META_ORIGINAL, META_SERVER}};
use common::{serve_mock, text_of, tool};
use mcp::client::Client;
use rmcp::{model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Tool}, RoleClient, ServiceExt};

fn proxied(server: &str, original: &str, exposed: &str) -> Tool {
    let mut t = Tool::new(exposed.to_string(), format!("{original} desc"), Arc::new(serde_json::Map::new()));
    let mut meta = rmcp::model::MetaObject::default();
    meta.0.insert(META_SERVER.into(), server.into());
    meta.0.insert(META_ORIGINAL.into(), original.into());
    t.meta = Some(meta);
    t
}

fn catalog() -> Vec<Tool> {
    vec![
        proxied("slack", "search_messages", "chat-search_messages"),
        proxied("slack", "post", "chat-post"),
        proxied("slack", "conversations_history", "chat-history"),
        proxied("gtasks", "list_tasks", "gtasks-list_tasks"),
        Tool::new("stray", "no meta", Arc::new(serde_json::Map::new())),
    ]
}

fn profile(yaml: &str) -> Profile {
    let c = mcp::config::from_value(&rafa_config::Layer::parse("t", yaml).unwrap().value).unwrap();
    c.profiles["p"].clone()
}

fn names(v: &mcp::client::View) -> Vec<&str> {
    v.tools.iter().map(|t| t.name.as_ref()).collect()
}

#[test]
fn profile_selects_servers_and_matches_original_names() {
    let v = view(&catalog(), &profile("profiles:\n  p:\n    servers:\n      slack: { include: [\"search_*\", conversations_history] }\n")).unwrap();
    assert_eq!(names(&v), vec!["chat-history", "chat-search_messages"]);
    assert_eq!(v.reverse["chat-history"], "chat-history");
}

#[test]
fn unlisted_servers_and_meta_less_tools_are_hidden() {
    let v = view(&catalog(), &profile("profiles:\n  p:\n    servers:\n      gtasks: {}\n")).unwrap();
    assert_eq!(names(&v), vec!["gtasks-list_tasks"]);
}

#[test]
fn client_rename_is_final_and_describe_overrides() {
    let p = profile("profiles:\n  p:\n    servers:\n      slack:\n        include: [search_messages]\n        rename: { search_messages: find }\n        describe: { search_messages: Find things. }\n");
    let v = view(&catalog(), &p).unwrap();
    assert_eq!(names(&v), vec!["find"]);
    assert_eq!(v.reverse["find"], "chat-search_messages");
    assert!(!v.reverse.contains_key("chat-search_messages"));
    assert_eq!(v.tools[0].description.as_deref(), Some("Find things."));
    assert!(v.tools[0].meta.as_ref().unwrap().0.contains_key(META_ORIGINAL));
}

#[test]
fn exclude_hides_and_wins() {
    let v = view(&catalog(), &profile("profiles:\n  p:\n    servers:\n      slack: { exclude: [post] }\n")).unwrap();
    assert_eq!(names(&v), vec!["chat-history", "chat-search_messages"]);
}

#[test]
fn renames_that_collide_are_errors() {
    let p = profile("profiles:\n  p:\n    servers:\n      slack: { rename: { post: chat-history } }\n");
    assert!(matches!(view(&catalog(), &p), Err(ViewError::Collision { ref name, .. }) if name == "chat-history"));
}

#[test]
fn inline_profile_drives_the_view() {
    let p = inline_profile(&["slack".into(), "gtasks".into()], &["slack:search_*".into(), "gtasks:*".into()], &[], false, None).unwrap();
    assert_eq!(names(&view(&catalog(), &p).unwrap()), vec!["chat-search_messages", "gtasks-list_tasks"]);
}

#[test]
fn toonify_encodes_json_text_and_leaves_prose() {
    let r = CallToolResult::success(vec![
        ContentBlock::text(r#"{"rows":[{"id":1,"t":"a"},{"id":2,"t":"b"}]}"#),
        ContentBlock::text("plain words"),
        ContentBlock::text("42"),
    ]);
    let out = toonify(r);
    let texts: Vec<String> = out.content.iter().map(|c| c.as_text().unwrap().text.clone()).collect();
    assert_eq!(toon::decode_default(&texts[0]).unwrap(), serde_json::json!({"rows":[{"id":1,"t":"a"},{"id":2,"t":"b"}]}));
    assert!(texts[0].contains("rows[2]{id,t}"), "{}", texts[0]);
    assert_eq!(texts[1], "plain words");
    assert_eq!(texts[2], "42");
}

// -- the effectful client: agent <-duplex-> Client <-unix-> proxy <-unix-> mock --------------

const PROXY: &str = "servers:\n  slack: { prefix: chat }\n";

struct Stack {
    dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    proxy: mcp::proxy::Proxy,
}

async fn start_proxy(dir: &std::path::Path) -> mcp::proxy::Proxy {
    let config = mcp::config::from_value(&rafa_config::Layer::parse("t", PROXY).unwrap().value).unwrap();
    let proxy = mcp::proxy::Proxy::new(config);
    proxy.sync_pool(&dir.join("servers")).await;
    let socket = dir.join("proxy.sock");
    let _ = std::fs::remove_file(&socket);
    tokio::spawn(proxy.clone().run(tokio::net::UnixListener::bind(&socket).unwrap()));
    proxy
}

async fn stack() -> Stack {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("servers")).unwrap();
    serve_mock(&dir.path().join("servers/slack.sock"), vec![tool("search_messages"), tool("post"), tool("json")]);
    let proxy = start_proxy(dir.path()).await;
    let socket = dir.path().join("proxy.sock");
    Stack { dir, socket, proxy }
}

async fn agent_via_client(socket: &std::path::Path, p: Profile) -> rmcp::service::RunningService<RoleClient, ()> {
    let (a, b) = tokio::io::duplex(64 * 1024);
    let client = Client::new(socket.to_path_buf(), p);
    tokio::spawn(async move { client.serve_stdio_like(b).await });
    ().serve(a).await.unwrap()
}

fn call(name: &str) -> CallToolRequestParams {
    CallToolRequestParams::new(name.to_string())
}

#[tokio::test]
async fn the_agent_sees_the_profile_and_calls_through_renames() {
    let s = stack().await;
    let p = profile("profiles:\n  p:\n    servers:\n      slack: { include: [\"search_*\"], rename: { search_messages: find } }\n");
    let agent = agent_via_client(&s.socket, p).await;
    let names: Vec<_> = agent.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["find"]);
    let CallToolResponse::Complete(r) = agent.call_tool_once(call("find")).await.unwrap() else { panic!() };
    assert!(text_of(&r).starts_with("called search_messages"), "{}", text_of(&r));
    assert!(agent.call_tool_once(call("chat-post")).await.is_err());
    assert!(agent.call_tool_once(call("chat-search_messages")).await.is_err());
}

#[tokio::test]
async fn toon_profiles_get_toon_results() {
    let s = stack().await;
    let p = inline_profile(&["slack".into()], &[], &[], true, None).unwrap();
    let agent = agent_via_client(&s.socket, p).await;
    let CallToolResponse::Complete(r) = agent.call_tool_once(call("chat-json")).await.unwrap() else { panic!() };
    assert!(text_of(&r).contains("rows[2]{id,t}"), "{}", text_of(&r));
}

#[tokio::test]
async fn the_client_reconnects_after_the_proxy_restarts() {
    let s = stack().await;
    let p = inline_profile(&["slack".into()], &[], &[], false, None).unwrap();
    let agent = agent_via_client(&s.socket, p).await;
    assert_eq!(agent.list_all_tools().await.unwrap().len(), 3);
    s.proxy.shutdown(Duration::from_millis(100)).await;
    let _restarted = start_proxy(s.dir.path()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let CallToolResponse::Complete(r) = agent.call_tool_once(call("chat-post")).await.unwrap() else { panic!() };
    assert!(text_of(&r).starts_with("called post"));
}

#[tokio::test]
async fn a_client_timeout_bounds_the_call() {
    let s = stack().await;
    let _ = &s.proxy;
    serve_mock(&s.dir.path().join("servers/slow.sock"), vec![tool("slow")]);
    s.proxy.sync_pool(&s.dir.path().join("servers")).await;
    let p = inline_profile(&["slow".into()], &[], &[], false, Some(1)).unwrap();
    let agent = agent_via_client(&s.socket, p).await;
    let CallToolResponse::Complete(r) = agent.call_tool_once(call("slow-slow")).await.unwrap() else { panic!() };
    assert_eq!(r.is_error, Some(true));
}

#[derive(Clone, Default)]
struct ListChanged(Arc<tokio::sync::Notify>);

impl rmcp::ClientHandler for ListChanged {
    async fn on_tool_list_changed(&self, _: rmcp::service::NotificationContext<RoleClient>) {
        self.0.notify_one();
    }
}

#[tokio::test]
async fn a_server_tool_change_reaches_the_agent_through_the_client() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("servers")).unwrap();
    let web = serve_mock(&dir.path().join("servers/web.sock"), vec![tool("browse")]);
    let _proxy = start_proxy(dir.path()).await;
    let (a, b) = tokio::io::duplex(64 * 1024);
    let client = Client::new(dir.path().join("proxy.sock"), inline_profile(&["web".into()], &[], &[], false, None).unwrap());
    tokio::spawn(async move { client.serve_stdio_like(b).await });
    let changed = ListChanged::default();
    let agent = changed.clone().serve(a).await.unwrap();
    assert_eq!(agent.list_all_tools().await.unwrap().len(), 1);
    web.set_tools(vec![tool("browse"), tool("screenshot")]).await;
    tokio::time::timeout(Duration::from_secs(3), changed.0.notified()).await.expect("list_changed reached the agent");
    let names: Vec<_> = agent.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["web-browse", "web-screenshot"]);
}

#[tokio::test]
async fn a_missing_proxy_is_an_error_not_a_hang() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = tokio::io::duplex(64 * 1024);
    let client = Client::new(dir.path().join("absent.sock"), inline_profile(&["x".into()], &[], &[], false, None).unwrap());
    tokio::spawn(async move { client.serve_stdio_like(b).await });
    let agent = ().serve(a).await.unwrap();
    let err = agent.list_all_tools().await.unwrap_err().to_string();
    assert!(err.contains("mcp proxy unavailable"), "{err}");
}
