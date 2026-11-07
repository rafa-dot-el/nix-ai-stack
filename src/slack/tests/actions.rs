mod common;

use serde_json::json;
use slack::{actions::{delete, ephemeral, react, send_spec, update}, client::Client, config::from_value, read::Slack, spec::from_yaml};

fn ok(v: serde_json::Value) -> (u16, serde_json::Value) {
    let mut v = v;
    v["ok"] = true.into();
    (200, v)
}

fn cfg() -> slack::config::Config {
    let y = "policy:\n  default: deny\n  rules:\n    - { match: { self_dm: true }, action: live }\n    - { match: { channel: \"notes-*\" }, action: live }\n    - { match: { channel: \"team-*\" }, action: draft }\n";
    from_value(Some(&rafa_config::Layer::parse("s", y).unwrap().value)).unwrap()
}

fn slack(s: &common::Stub, token: &str) -> Slack {
    Slack::new(Client::new(token, Some("c".into()), &s.base).unwrap())
}

#[tokio::test]
async fn a_dry_run_shows_the_decision_and_the_requests_and_sends_nothing() {
    let s = common::stub(vec![ok(json!({"user_id": "UME"})), ok(json!({"channel": {"id": "DME"}}))]).await;
    let r = send_spec(&slack(&s, "xoxp-1"), &cfg(), &from_yaml("to: me\ntext: hi\n").unwrap(), true, &[], "u").await.unwrap();
    assert_eq!(r["action"], "live");
    assert_eq!(r["rule"], 0);
    assert_eq!(r["channel"], "DME");
    assert_eq!(r["requests"][0]["chat.postMessage"]["text"], "hi");
    assert_eq!(s.seen().len(), 2, "only the resolution, nothing sent");
}

#[tokio::test]
async fn live_draft_and_denied_destinations() {
    let s = common::stub(vec![ok(json!({"channels": [{"id": "C1", "name": "notes-daily"}, {"id": "C2", "name": "team-x"}, {"id": "C3", "name": "random"}]})), ok(json!({"ts": "1.1"}))]).await;
    let sl = slack(&s, "xoxp-1");
    let sent = send_spec(&sl, &cfg(), &from_yaml("to: \"#notes-daily\"\ntext: hi\n").unwrap(), false, &[], "u").await.unwrap();
    assert_eq!(sent["ts"], json!(["1.1"]));
    let err = send_spec(&sl, &cfg(), &from_yaml("to: \"#team-x\"\ntext: hi\n").unwrap(), false, &[], "u").await.unwrap_err();
    assert!(err.contains("xoxc"), "a draft channel with a non-session token: {err}");
    let err = send_spec(&sl, &cfg(), &from_yaml("to: \"#team-x\"\ntext: hi\nmode: live\n").unwrap(), false, &[], "u").await.unwrap_err();
    assert!(err.contains("only allows a draft"), "{err}");
    let err = send_spec(&sl, &cfg(), &from_yaml("to: \"#random\"\ntext: hi\n").unwrap(), false, &[], "u").await.unwrap_err();
    assert!(err.contains("denied"), "{err}");
    let err = send_spec(&sl, &cfg(), &from_yaml("to: \"no such\"\ntext: hi\n").unwrap(), false, &[], "u").await.unwrap_err();
    assert!(err.contains("not a target"), "{err}");
}

#[tokio::test]
async fn edits_deletes_and_reactions_need_a_live_verdict() {
    let s = common::stub(vec![
        ok(json!({"channel": {"id": "C1", "name": "notes-daily"}})),
        ok(json!({"ts": "1712345678.123456"})),
        ok(json!({"channel": {"id": "C1", "name": "notes-daily"}})),
        ok(json!({})),
        ok(json!({"channel": {"id": "C1", "name": "notes-daily"}})),
        ok(json!({})),
        ok(json!({"channel": {"id": "C2", "name": "team-x"}})),
    ])
    .await;
    let sl = slack(&s, "xoxp-1");
    let link = "https://acme.slack.com/archives/C1/p1712345678123456";
    update(&sl, &cfg(), link, Some("**fixed**"), None).await.unwrap();
    react(&sl, &cfg(), link, ":+1:").await.unwrap();
    delete(&sl, &cfg(), link).await.unwrap();
    let seen = s.seen();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[1].body).unwrap(), json!({"channel": "C1", "ts": "1712345678.123456", "markdown_text": "**fixed**"}));
    assert!(seen[3].target.ends_with("/reactions.add") && seen[3].body.contains("name=%2B1"), "{}", seen[3].body);
    assert!(seen[5].target.ends_with("/chat.delete"));
    let err = delete(&sl, &cfg(), "https://acme.slack.com/archives/C2/p1712345678123456").await.unwrap_err();
    assert!(err.contains("live"), "a draft-only channel cannot be edited: {err}");
}

#[tokio::test]
async fn ephemeral_messages() {
    let s = common::stub(vec![ok(json!({"channels": [{"id": "C1", "name": "notes-daily"}]})), ok(json!({"message_ts": "2.2"}))]).await;
    let r = ephemeral(&slack(&s, "xoxp-1"), &cfg(), "#notes-daily", "U7", "only you").await.unwrap();
    assert_eq!(r["message_ts"], "2.2");
    assert!(s.seen()[1].body.contains("user=U7"));
}

#[tokio::test]
async fn cancelling_drafts_and_schedules_goes_through_the_policy() {
    use slack::actions::{draft_delete, schedule_delete};
    let s = common::stub(vec![
        ok(json!({"channel": {"id": "C2", "name": "team-x"}})),
        ok(json!({})),
        ok(json!({"channel": {"id": "C3", "name": "random"}})),
        ok(json!({"channel": {"id": "C2", "name": "team-x"}})),
        ok(json!({"channel": {"id": "C1", "name": "notes-daily"}})),
        ok(json!({})),
    ])
    .await;
    let sl = slack(&s, "xoxc-1");
    draft_delete(&sl, &cfg(), "Dr1", "C2").await.unwrap();
    assert!(draft_delete(&sl, &cfg(), "Dr2", "C3").await.unwrap_err().contains("Deny"), "a denied channel");
    assert!(schedule_delete(&sl, &cfg(), "Q1", "C2").await.unwrap_err().contains("Draft"), "schedules exist only where live");
    schedule_delete(&sl, &cfg(), "Q2", "C1").await.unwrap();
    let seen = s.seen();
    assert_eq!(seen.len(), 6);
    assert!(seen[1].target.ends_with("/drafts.delete") && seen[1].body.contains("id=Dr1"));
    assert!(seen[5].target.ends_with("/chat.deleteScheduledMessage") && seen[5].body.contains("scheduled_message_id=Q2"));
}
