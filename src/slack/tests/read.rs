mod common;

use serde_json::json;
use slack::{client::Client, read::{channel, display_name, user, Slack}};

fn slack(s: &common::Stub) -> Slack {
    Slack::new(Client::new("xoxp-1", None, &s.base).unwrap())
}

fn ok(v: serde_json::Value) -> (u16, serde_json::Value) {
    let mut v = v;
    v["ok"] = true.into();
    (200, v)
}

fn raw_user(id: &str, name: &str, display: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "real_name": format!("{name} Real"), "is_bot": false, "profile": {"display_name": display, "email": format!("{name}@x.org"), "title": "T"}})
}

#[test]
fn shapes() {
    let u = user(&raw_user("U1", "ada", "Ada L"));
    assert_eq!(u, json!({"id": "U1", "name": "ada", "real_name": "ada Real", "display_name": "Ada L", "email": "ada@x.org", "title": "T", "is_bot": false, "deleted": false}));
    assert_eq!(display_name(&u, "U1"), "Ada L");
    assert_eq!(display_name(&user(&raw_user("U2", "bob", "")), "U2"), "bob Real");
    assert_eq!(display_name(&json!(null), "U3"), "U3");
    let c = channel(&json!({"id": "C1", "name": "general", "is_private": false, "is_member": true, "topic": {"value": "t"}, "purpose": {"value": "p"}, "num_members": 3}));
    assert_eq!(c, json!({"id": "C1", "name": "general", "is_private": false, "is_member": true, "topic": "t", "purpose": "p", "num_members": 3}));
}

#[tokio::test]
async fn messages_carry_names_from_the_cache() {
    let s = common::stub(vec![
        ok(json!({"members": [raw_user("U1", "ada", "Ada L"), {"id": "B1", "is_bot": true, "profile": {}}]})),
        ok(json!({"messages": [{"user": "U1", "text": "hi", "ts": "1.1", "reactions": [{"name": "+1", "count": 2, "users": ["x"]}]}], "has_more": false})),
    ])
    .await;
    let sl = slack(&s);
    let users = sl.users_list(1000, false).await.unwrap();
    assert_eq!(users["count"], 1, "bots excluded by default");
    let h = sl.history("C1", 50, Some("1.0"), None).await.unwrap();
    assert_eq!(h["messages"][0], json!({"user": "U1", "display_name": "Ada L", "text": "hi", "ts": "1.1", "thread_ts": "", "reply_count": 0, "reactions": [{"name": "+1", "count": 2}], "subtype": ""}));
    assert_eq!(h["count"], 1);
    assert!(s.seen()[1].body.contains("channel=C1") && s.seen()[1].body.contains("oldest=1.0"));
}

#[tokio::test]
async fn users_info_is_cached_and_cache_can_be_cleared() {
    let s = common::stub(vec![ok(json!({"user": raw_user("U1", "ada", "Ada L")})), ok(json!({"user": raw_user("U1", "ada", "Ada L")}))]).await;
    let sl = slack(&s);
    sl.users_info("U1").await.unwrap();
    sl.users_info("U1").await.unwrap();
    assert_eq!(s.seen().len(), 1, "the second is from the cache");
    assert_eq!(sl.cache_stats()["users"], 1);
    assert_eq!(sl.clear_cache("users")["cleared"], json!(["users"]));
    sl.users_info("U1").await.unwrap();
    assert_eq!(s.seen().len(), 2);
}

#[tokio::test]
async fn find_user_by_id_email_and_name() {
    let s = common::stub(vec![
        ok(json!({"members": [raw_user("U1", "ada", "Ada L"), raw_user("U2", "adam", "")]})),
        ok(json!({"user": raw_user("U9", "zed", "Z")})),
    ])
    .await;
    let sl = slack(&s);
    sl.users_list(1000, false).await.unwrap();
    assert_eq!(sl.find_user("ada@x.org").await.unwrap()["users"][0]["id"], "U1", "from the cache");
    assert_eq!(sl.find_user("ada").await.unwrap()["users"].as_array().unwrap().len(), 2, "substring of name, display or real name");
    assert_eq!(sl.find_user("zed@x.org").await.unwrap()["users"][0]["id"], "U9", "users.lookupByEmail");
    assert!(sl.find_user("nobody").await.unwrap()["error"].as_str().unwrap().contains("nobody"));
    assert!(s.seen()[1].target.ends_with("/users.lookupByEmail"));
}

#[tokio::test]
async fn a_users_channels_come_from_users_conversations() {
    let s = common::stub(vec![
        ok(json!({"channels": [{"id": "C1", "name": "big", "is_private": false}], "response_metadata": {"next_cursor": "n"}})),
        ok(json!({"channels": [{"id": "C2", "name": "small", "is_private": true}]})),
    ])
    .await;
    let r = slack(&s).find_user_channels("U1", "public_channel,private_channel").await.unwrap();
    assert_eq!(r, json!({"channels": [{"channel_id": "C1", "name": "big", "is_private": false}, {"channel_id": "C2", "name": "small", "is_private": true}], "count": 2}));
    let seen = s.seen();
    assert_eq!(seen.len(), 2, "one paged call, not one per channel");
    assert!(seen[0].target.ends_with("/users.conversations") && seen[0].body.contains("user=U1"));
}

#[tokio::test]
async fn lists_info_members_replies_search_and_resolution() {
    let s = common::stub(vec![
        ok(json!({"channels": [{"id": "C1", "name": "general"}]})),
        ok(json!({"channel": {"id": "C1", "name": "general", "topic": {"value": "t"}, "purpose": {"value": ""}, "creator": "U1", "created": 5, "num_members": 9}})),
        ok(json!({"members": ["U1", "U7"]})),
        ok(json!({"messages": [{"user": "U7", "text": "r", "ts": "2.2"}]})),
        ok(json!({"messages": {"matches": [{"user": "U1", "text": "found", "ts": "3.3", "channel": {"id": "C1", "name": "general"}, "permalink": "p"}], "total": 1, "paging": {"page": 1, "pages": 1}}})),
        ok(json!({"user": raw_user("U7", "eve", "Eve")})),
    ])
    .await;
    let sl = slack(&s);
    assert_eq!(sl.conversations_list("public_channel", 200).await.unwrap()["channels"][0]["name"], "general");
    assert_eq!(sl.conversations_info("C1").await.unwrap()["creator"], "U1");
    assert_eq!(sl.conversations_members("C1", 200).await.unwrap()["members"][1], json!({"user_id": "U7", "display_name": "U7"}));
    assert_eq!(sl.replies("C1", "2.2", 200).await.unwrap()["messages"][0]["text"], "r");
    let found = sl.search("in:#general found", 20, 1).await.unwrap();
    assert_eq!(found["matches"][0], json!({"user": "U1", "text": "found", "ts": "3.3", "channel": "general", "channel_id": "C1", "permalink": "p"}));
    assert_eq!((found["total"].clone(), found["pages"].clone()), (json!(1), json!(1)));
    assert_eq!(sl.resolve_user_ids(&["U7".into()]).await.unwrap(), json!({"U7": "Eve"}));
}

#[tokio::test]
async fn enterprise_grids_fall_back_to_client_counts() {
    let s = common::stub(vec![
        (200, json!({"ok": false, "error": "enterprise_is_restricted"})),
        ok(json!({"channels": [{"id": "C1"}, {"id": "C2"}]})),
        ok(json!({"channel": {"id": "C1", "name": "a"}})),
        (200, json!({"ok": false, "error": "channel_not_found"})),
    ])
    .await;
    let r = slack(&s).conversations_list("public_channel", 200).await.unwrap();
    assert_eq!(r["count"], 1, "a failing channel is skipped");
    assert!(s.seen()[1].target.ends_with("/client.counts"));
}
