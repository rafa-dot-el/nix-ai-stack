mod common;

use serde_json::json;
use slack::{
    client::Client,
    config::from_value,
    policy::{Action, Destination},
    read::Slack,
    send::{execute, plan, resolve, Body, File, Options},
    target::Target,
};

fn ok(v: serde_json::Value) -> (u16, serde_json::Value) {
    let mut v = v;
    v["ok"] = true.into();
    (200, v)
}

fn slack(s: &common::Stub, token: &str) -> Slack {
    Slack::new(Client::new(token, Some("d-cookie".into()), &s.base).unwrap())
}

#[tokio::test]
async fn targets_resolve_to_destinations() {
    let s = common::stub(vec![
        ok(json!({"user_id": "UME"})),
        ok(json!({"channel": {"id": "DME"}})),
        ok(json!({"members": [{"id": "U2", "name": "ada", "profile": {"display_name": "Ada"}}]})),
        ok(json!({"channel": {"id": "DADA"}})),
        ok(json!({"channels": [{"id": "C9", "name": "notes-daily"}]})),
        ok(json!({"channel": {"id": "C5", "name": "general"}})),
    ])
    .await;
    let sl = slack(&s, "xoxp-1");
    assert_eq!(resolve(&sl, &Target::Me).await.unwrap(), Destination { channel_id: "DME".into(), name: None, self_dm: true });
    assert_eq!(resolve(&sl, &Target::User("ada".into())).await.unwrap(), Destination { channel_id: "DADA".into(), name: None, self_dm: false });
    assert_eq!(resolve(&sl, &Target::ChannelName("notes-daily".into())).await.unwrap(), Destination { channel_id: "C9".into(), name: Some("notes-daily".into()), self_dm: false });
    assert_eq!(resolve(&sl, &Target::ChannelId("C5".into())).await.unwrap().name.as_deref(), Some("general"));
    assert!(resolve(&sl, &Target::ChannelId("DME".into())).await.unwrap().self_dm, "own DM, from the cached self DM");
    let t: Vec<String> = s.seen().iter().map(|x| x.target.clone()).collect();
    assert_eq!(t, vec!["/auth.test", "/conversations.open", "/users.list", "/conversations.open", "/conversations.list", "/conversations.info"]);
    assert!(s.seen()[3].body.contains("users=U2"));
}

fn temp_file(tag: &str, name: &str, content: &str) -> File {
    let dir = std::env::temp_dir().join(format!("slack-exec-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), content).unwrap();
    File { path: dir.join(name).display().to_string(), name: name.into(), size: content.len() as u64, title: None, alt: None, snippet_type: None }
}

#[tokio::test]
async fn a_live_post_with_a_file_uploads_shares_and_posts() {
    let up = common::stub(vec![(200, json!({}))]).await;
    let s = common::stub(vec![
        ok(json!({"upload_url": format!("{}/upload-here", up.base), "file_id": "F1"})),
        ok(json!({"files": [{"id": "F1"}]})),
        ok(json!({"ts": "9.9", "channel": "C1"})),
    ])
    .await;
    let sl = slack(&s, "xoxp-1");
    let body = Body { text: Some("hi".into()), files: vec![temp_file("live", "a.txt", "abc")], ..Default::default() };
    let steps = plan(&body, "C1", Action::Live, &Options { client_msg_id: "u".into(), ..Default::default() }, &from_value(None).unwrap(), false).unwrap();
    let r = execute(&sl, steps, "C1", None).await.unwrap();
    assert_eq!((r["file_ids"].clone(), r["ts"].clone()), (json!(["F1"]), json!(["9.9"])));
    let seen = s.seen();
    assert_eq!(seen[0].target, "/files.getUploadURLExternal");
    assert!(seen[0].body.contains("filename=a.txt") && seen[0].body.contains("length=3"));
    let upload = &up.seen()[0];
    assert_eq!((upload.method.as_str(), upload.target.as_str(), upload.body.as_str()), ("POST", "/upload-here", "abc"), "the bytes are POSTed");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[1].body).unwrap()["channel_id"], "C1", "live: shared into the channel");
    assert_eq!(seen[2].target, "/chat.postMessage");
}

#[tokio::test]
async fn a_draft_attaches_private_uploads_by_id() {
    let up = common::stub(vec![(200, json!({}))]).await;
    let s = common::stub(vec![
        ok(json!({"upload_url": format!("{}/u", up.base), "file_id": "F7"})),
        ok(json!({"files": [{"id": "F7"}]})),
        ok(json!({"draft": {"id": "Dr1"}})),
    ])
    .await;
    let sl = slack(&s, "xoxc-1");
    let body = Body { text: Some("draft".into()), files: vec![temp_file("draft", "b.txt", "xyz")], ..Default::default() };
    let steps = plan(&body, "C2", Action::Draft, &Options { client_msg_id: "u".into(), ..Default::default() }, &from_value(None).unwrap(), true).unwrap();
    let r = execute(&sl, steps, "C2", None).await.unwrap();
    assert_eq!(r["draft_id"], "Dr1");
    let seen = s.seen();
    let complete: serde_json::Value = serde_json::from_str(&seen[1].body).unwrap();
    assert!(complete.get("channel_id").is_none(), "nothing is shared before the draft exists: {complete}");
    assert_eq!(seen[2].target, "/drafts.create");
    assert!(seen[2].body.contains("file_ids=%5B%22F7%22%5D"), "{}", seen[2].body);
    assert!(seen[2].head.contains("d=d-cookie"));
}

#[tokio::test]
async fn a_failing_upload_stops_before_the_message() {
    let up = common::stub(vec![(500, json!({}))]).await;
    let s = common::stub(vec![ok(json!({"upload_url": format!("{}/u", up.base), "file_id": "F1"}))]).await;
    let sl = slack(&s, "xoxp-1");
    let body = Body { text: Some("hi".into()), files: vec![temp_file("fail", "c.txt", "q")], ..Default::default() };
    let steps = plan(&body, "C1", Action::Live, &Options { client_msg_id: "u".into(), ..Default::default() }, &from_value(None).unwrap(), false).unwrap();
    assert!(execute(&sl, steps, "C1", None).await.unwrap_err().contains("upload"));
    assert_eq!(s.seen().len(), 1, "no completion, no post");
}
