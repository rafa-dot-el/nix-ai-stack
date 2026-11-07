use std::collections::HashMap;

use serde_json::json;
use slack::analysis::{action_items, activity, default_keywords, topics};

fn names() -> HashMap<String, String> {
    HashMap::from([("U1".to_string(), "Ada".to_string())])
}

#[test]
fn activity_summary() {
    // 2026-06-01T09:xx UTC is hour 9; one reply in a thread; reactions on U1's message
    let msgs = vec![
        json!({"user": "U1", "ts": "1780304400.000100", "reactions": [{"name": "+1", "count": 3}]}),
        json!({"user": "U1", "ts": "1780304500.000100", "thread_ts": "1780304400.000100"}),
        json!({"user": "U2", "ts": "1780308000.000100"}),
        json!({"ts": "1780308100.000100"}),
    ];
    let a = activity(&msgs, 2, 10, &names());
    assert_eq!(a["message_count_total"], 4);
    assert_eq!(a["messages_per_day"], 2.0);
    assert_eq!(a["top_posters"][0], json!({"user": "U1", "display_name": "Ada", "message_count": 2, "percentage": 50.0}));
    assert_eq!(a["top_posters"][1]["display_name"], "U2");
    assert_eq!(a["peak_hours"][0], json!({"hour": 9, "count": 2}));
    assert_eq!(a["timezone"], "UTC");
    assert_eq!(a["thread_ratio"], 0.25);
    assert_eq!(a["reaction_leaders"][0], json!({"user": "U1", "display_name": "Ada", "reactions_received": 3}));
    let empty = activity(&[], 30, 10, &names());
    assert_eq!(empty, json!({"top_posters": [], "message_count_total": 0, "messages_per_day": 0.0, "peak_hours": [], "timezone": "UTC", "thread_ratio": 0.0, "reaction_leaders": []}));
}

#[test]
fn topics_links_and_active_threads() {
    let msgs = vec![
        json!({"text": "Deploy the release tonight, release notes at https://x.org/notes", "ts": "1", "reply_count": 4}),
        json!({"text": "release window confirmed with <https://x.org/notes>", "ts": "2"}),
        json!({"text": "this that them with", "ts": "3"}),
    ];
    let t = topics(&msgs);
    assert_eq!(t["topics"][0], json!({"topic_keyword": "release", "message_count": 3}));
    assert!(!t["topics"].as_array().unwrap().iter().any(|x| x["topic_keyword"] == "that" || x["topic_keyword"] == "with"), "stop words");
    assert_eq!(t["links_shared"][0], json!({"url": "https://x.org/notes", "count": 2}));
    assert_eq!(t["active_threads"], json!([{"ts": "1", "text_preview": "Deploy the release tonight, release notes at https://x.org/notes", "reply_count": 4}]));
}

#[test]
fn action_items_by_keyword_or_request() {
    let msgs = vec![
        json!({"user": "U1", "text": "TODO: rotate the keys", "ts": "1"}),
        json!({"user": "U2", "text": "could you review the migration plan today?", "ts": "2", "thread_ts": "1"}),
        json!({"user": "U2", "text": "please do", "ts": "3"}),
        json!({"user": "U2", "text": "nice weather", "ts": "4"}),
    ];
    let a = action_items(&msgs, &default_keywords(), "C1", &names());
    assert_eq!(a["count"], 2, "\"please do\" is too short to be a request");
    assert_eq!(a["action_items"][0], json!({"ts": "1", "user": "U1", "display_name": "Ada", "text": "TODO: rotate the keys", "thread_ts": "", "channel": "C1"}));
    assert_eq!(action_items(&msgs, &["weather".into()], "C1", &names())["count"], 2, "custom keywords, requests still count");
}
