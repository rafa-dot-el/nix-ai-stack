mod common;

use std::{collections::BTreeMap, sync::Arc};

use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use serde_json::json;
use slack::{mcp::Server, setup::{load, uuid_v4, NOT_CONFIGURED}};

fn home(tag: &str, yaml: &str) -> (std::path::PathBuf, BTreeMap<String, String>) {
    let d = std::env::temp_dir().join(format!("slack-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    std::fs::write(d.join("token"), "xoxp-test\n").unwrap();
    std::fs::write(d.join("cfg/rafa/slack.yaml"), yaml.replace("TOKEN", &d.join("token").display().to_string())).unwrap();
    let env = BTreeMap::from([("HOME".to_string(), d.display().to_string()), ("XDG_CONFIG_HOME".to_string(), d.join("cfg").display().to_string())]);
    (d, env)
}

async fn agent(server: Server) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (a, b) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(r) = server.serve(b).await {
            let _ = r.waiting().await;
        }
    });
    ().serve(a).await.unwrap()
}

async fn call(a: &rmcp::service::RunningService<rmcp::RoleClient, ()>, name: &str, args: serde_json::Value) -> (bool, serde_json::Value) {
    let mut p = CallToolRequestParams::new(name.to_string());
    p.arguments = args.as_object().cloned();
    let CallToolResponse::Complete(r) = a.call_tool_once(p).await.unwrap() else { panic!() };
    let text: String = r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect();
    (r.is_error != Some(true), serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))
}

#[test]
fn uuids_are_version_4() {
    let u = uuid_v4([0xff; 16]);
    assert_eq!(u, "ffffffff-ffff-4fff-bfff-ffffffffffff");
    assert_eq!(uuid_v4([0; 16]), "00000000-0000-4000-8000-000000000000");
    assert_ne!(slack::setup::new_msg_id(), slack::setup::new_msg_id());
}

#[tokio::test]
async fn tools_are_named_and_annotated() {
    let (_, env) = home("names", "");
    let a = agent(Server::new(Arc::new(load(&env, "http://127.0.0.1:1").unwrap()))).await;
    let tools = a.list_all_tools().await.unwrap();
    let ro = |n: &str| tools.iter().find(|t| t.name == n).unwrap_or_else(|| panic!("{n}")).annotations.as_ref().unwrap().read_only_hint == Some(true);
    let destructive = |n: &str| tools.iter().find(|t| t.name == n).unwrap().annotations.as_ref().unwrap().destructive_hint == Some(true);
    for read in ["conversations_list", "conversations_info", "conversations_members", "conversations_history", "conversations_replies", "dump_recent_messages",
        "search_messages", "users_list", "users_info", "find_user", "find_user_channels", "resolve_user_ids", "channel_activity_summary", "my_interactions",
        "channel_topics_summary", "extract_action_items", "status", "slack_resolve", "slack_draft_list", "slack_schedule_list"] {
        assert!(ro(read), "{read} is read-only");
    }
    for write in ["slack_send", "slack_update", "slack_react", "slack_ephemeral", "clear_cache"] {
        assert!(!ro(write) && !destructive(write), "{write}");
    }
    for d in ["slack_delete", "slack_draft_delete", "slack_schedule_delete"] {
        assert!(destructive(d), "{d}");
    }
}

#[tokio::test]
async fn without_a_token_tools_say_how_to_configure() {
    let (_, env) = home("none", "");
    let a = agent(Server::new(Arc::new(load(&env, "http://127.0.0.1:1").unwrap()))).await;
    let (ok, v) = call(&a, "users_list", json!({})).await;
    assert!(!ok);
    assert_eq!(v, json!(NOT_CONFIGURED));
    let (ok, status) = call(&a, "status", json!({})).await;
    assert!(ok && status["configured"] == false, "{status}");
}

#[tokio::test]
async fn send_dry_run_and_reads_through_the_server() {
    let s = common::stub(vec![
        (200, json!({"ok": true, "user_id": "UME"})),
        (200, json!({"ok": true, "channel": {"id": "DME"}})),
        (200, json!({"ok": true, "messages": [{"user": "UME", "text": "TODO: ship it", "ts": "1.1"}]})),
        (200, json!({"ok": true, "user": {"id": "UME", "name": "me", "profile": {"display_name": "Me"}}})),
    ])
    .await;
    let (_, env) = home("send", "credentials:\n  token: { file: \"TOKEN\" }\npolicy:\n  rules:\n    - { match: { self_dm: true }, action: live }\n");
    let a = agent(Server::new(Arc::new(load(&env, &s.base).unwrap()))).await;
    let (ok, v) = call(&a, "slack_send", json!({"to": "me", "text": "hi", "dry_run": true})).await;
    assert!(ok, "{v}");
    assert_eq!((v["action"].clone(), v["channel"].clone()), (json!("live"), json!("DME")));
    let (ok, items) = call(&a, "extract_action_items", json!({"channel": "DME"})).await;
    assert!(ok, "{items}");
    assert_eq!(items["action_items"][0]["display_name"], "Me");
    assert_eq!(s.seen()[0].auth, "Bearer xoxp-test", "the token came from the secret ref");
}

#[test]
fn the_voice_command_comes_from_rafa_voice_and_empty_means_none() {
    let (_, mut env) = home("voice", "");
    assert_eq!(load(&env, "http://127.0.0.1:1").unwrap().tts[0], "rafa-voice", "PATH lookup only when unset");
    env.insert("RAFA_VOICE".into(), "/nix/store/x-rafa-voice/bin/rafa-voice".into());
    assert_eq!(load(&env, "http://127.0.0.1:1").unwrap().tts, ["/nix/store/x-rafa-voice/bin/rafa-voice", "say", "-", "-o"]);
    env.insert("RAFA_VOICE".into(), String::new());
    assert!(load(&env, "http://127.0.0.1:1").unwrap().tts.is_empty(), "voice disabled in Nix");
}
