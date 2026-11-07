mod common;

use serde_json::json;
use slack::client::{Client, SlackError};

#[tokio::test]
async fn calls_post_forms_with_the_token_and_the_session_cookie() {
    let s = common::stub(vec![(200, json!({"ok": true, "user_id": "U1"}))]).await;
    let c = Client::new("xoxc-123", Some("cookie/with=chars".into()), &s.base).unwrap();
    assert!(c.is_session());
    let v = c.call("auth.test", &[("a", "1".into())]).await.unwrap();
    assert_eq!(v["user_id"], "U1");
    let seen = &s.seen()[0];
    assert_eq!((seen.method.as_str(), seen.target.as_str(), seen.body.as_str()), ("POST", "/auth.test", "a=1"));
    assert_eq!(seen.auth, "Bearer xoxc-123");
    assert!(seen.head.contains("d=cookie%2Fwith%3Dchars"), "{}", seen.head);
    assert!(!Client::new("xoxp-1", None, &s.base).unwrap().is_session());
}

#[tokio::test]
async fn ok_false_is_an_error_with_slacks_code() {
    let s = common::stub(vec![(200, json!({"ok": false, "error": "channel_not_found"}))]).await;
    let c = Client::new("xoxp-1", None, &s.base).unwrap();
    assert_eq!(c.call("conversations.info", &[]).await.unwrap_err(), SlackError::Api { method: "conversations.info".into(), code: "channel_not_found".into() });
}

#[tokio::test]
async fn rate_limits_are_retried_then_given_up() {
    let s = common::stub(vec![(429, json!({})), (200, json!({"ok": true}))]).await;
    let c = Client::new("xoxp-1", None, &s.base).unwrap();
    assert!(c.call("x", &[]).await.is_ok());
    assert_eq!(s.seen().len(), 2);
    let s = common::stub((0..6).map(|_| (429, json!({}))).collect()).await;
    let c = Client::new("xoxp-1", None, &s.base).unwrap();
    assert_eq!(c.call("x", &[]).await.unwrap_err(), SlackError::RateLimited { method: "x".into() });
}

#[tokio::test]
async fn json_calls_and_cursor_paging() {
    let s = common::stub(vec![
        (200, json!({"ok": true, "ts": "1.2"})),
        (200, json!({"ok": true, "channels": [{"id": "C1"}, {"id": "C2"}], "response_metadata": {"next_cursor": "n2"}})),
        (200, json!({"ok": true, "channels": [{"id": "C3"}], "response_metadata": {"next_cursor": ""}})),
    ])
    .await;
    let c = Client::new("xoxp-1", None, &s.base).unwrap();
    assert_eq!(c.call_json("chat.postMessage", &json!({"channel": "C1"})).await.unwrap()["ts"], "1.2");
    let all = c.paged("conversations.list", &[("limit", "200".into())], "channels", 10).await.unwrap();
    assert_eq!(all.len(), 3);
    let seen = s.seen();
    assert!(seen[0].head.to_ascii_lowercase().contains("content-type: application/json"));
    assert_eq!(seen[2].body, "limit=200&cursor=n2");
    let s = common::stub(vec![(200, json!({"ok": true, "channels": [{"id": "1"}, {"id": "2"}], "response_metadata": {"next_cursor": "more"}}))]).await;
    let c = Client::new("xoxp-1", None, &s.base).unwrap();
    assert_eq!(c.paged("conversations.list", &[], "channels", 2).await.unwrap().len(), 2, "stops at max");
    assert_eq!(s.seen().len(), 1);
}
