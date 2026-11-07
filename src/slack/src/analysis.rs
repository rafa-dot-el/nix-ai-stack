//! The analysis tools (the Python mcp_slack.tools.analysis, same outputs): pure functions
//! over messages, and the calls that fetch them.
use std::collections::HashMap;

use serde_json::Value;

/// Counts that remember first appearance, for `most_common` ties as Python orders them.
#[derive(Default)]
struct Counter<K: std::hash::Hash + Eq + Clone> {
    order: Vec<K>,
    counts: HashMap<K, u64>,
}

impl<K: std::hash::Hash + Eq + Clone> Counter<K> {
    fn add(&mut self, key: K, n: u64) {
        let e = self.counts.entry(key.clone()).or_insert_with(|| {
            self.order.push(key);
            0
        });
        *e += n;
    }
    fn most_common(&self, n: usize) -> Vec<(K, u64)> {
        let mut all: Vec<(usize, K, u64)> = self.order.iter().enumerate().map(|(i, k)| (i, k.clone(), self.counts[k])).collect();
        all.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        all.into_iter().take(n).map(|(_, k, c)| (k, c)).collect()
    }
}

fn round(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn name(names: &HashMap<String, String>, id: &str) -> String {
    names.get(id).cloned().unwrap_or_else(|| id.to_string())
}

/// Top posters, messages per day, peak hours (UTC), thread ratio, reaction leaders (pure).
/// `names`: user id -> display name.
pub fn activity(messages: &[Value], days: u64, top_n: usize, names: &HashMap<String, String>) -> Value {
    let total = messages.len();
    if total == 0 {
        return serde_json::json!({ "top_posters": [], "message_count_total": 0, "messages_per_day": 0.0, "peak_hours": [], "timezone": "UTC", "thread_ratio": 0.0, "reaction_leaders": [] });
    }
    let (mut posters, mut hours, mut reactions) = (Counter::default(), Counter::default(), Counter::default());
    let mut replies = 0;
    for m in messages {
        let uid = str_of(m, "user").to_string();
        if !uid.is_empty() {
            posters.add(uid.clone(), 1);
        }
        let secs: u64 = str_of(m, "ts").split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
        hours.add(secs % 86_400 / 3_600, 1);
        let thread = str_of(m, "thread_ts");
        if !thread.is_empty() && thread != str_of(m, "ts") {
            replies += 1;
        }
        for r in m.get("reactions").and_then(Value::as_array).into_iter().flatten() {
            reactions.add(uid.clone(), r.get("count").and_then(Value::as_u64).unwrap_or(0));
        }
    }
    let top: Vec<Value> = posters
        .most_common(top_n)
        .into_iter()
        .map(|(u, c)| serde_json::json!({ "user": u, "display_name": name(names, &u), "message_count": c, "percentage": round(c as f64 * 100.0 / total as f64, 1) }))
        .collect();
    let peaks: Vec<Value> = hours.most_common(5).into_iter().map(|(h, c)| serde_json::json!({ "hour": h, "count": c })).collect();
    let leaders: Vec<Value> = reactions
        .most_common(top_n)
        .into_iter()
        .map(|(u, c)| serde_json::json!({ "user": u, "display_name": name(names, &u), "reactions_received": c }))
        .collect();
    serde_json::json!({
        "top_posters": top,
        "message_count_total": total,
        "messages_per_day": round(total as f64 / days.max(1) as f64, 1),
        "peak_hours": peaks,
        "timezone": "UTC",
        "thread_ratio": round(replies as f64 / total as f64, 2),
        "reaction_leaders": leaders,
    })
}

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "shall", "can", "need", "dare", "ought", "used", "to", "of", "in", "for", "on", "with", "at", "by", "from",
    "it", "this", "that", "these", "those", "i", "you", "he", "she", "we", "they", "me", "him", "her", "us", "them", "my", "your",
    "his", "its", "our", "their", "and", "but", "or", "not", "no",
];

/// Frequent words (4+ letters, stop words out), shared links, threads with 3+ replies (pure).
pub fn topics(messages: &[Value]) -> Value {
    let url = regex::Regex::new(r"https?://[^\s>]+").expect("valid");
    let word = regex::Regex::new(r"\b[a-zA-Z]{4,}\b").expect("valid");
    let (mut words, mut links) = (Counter::default(), Counter::default());
    let mut threads = Vec::new();
    for m in messages {
        let text = str_of(m, "text");
        for u in url.find_iter(text) {
            links.add(u.as_str().to_string(), 1);
        }
        let lower = text.to_lowercase();
        for w in word.find_iter(&lower) {
            if !STOP_WORDS.contains(&w.as_str()) {
                words.add(w.as_str().to_string(), 1);
            }
        }
        let replies = m.get("reply_count").and_then(Value::as_u64).unwrap_or(0);
        if replies >= 3 && threads.len() < 10 {
            threads.push(serde_json::json!({ "ts": str_of(m, "ts"), "text_preview": text.chars().take(100).collect::<String>(), "reply_count": replies }));
        }
    }
    serde_json::json!({
        "topics": words.most_common(15).into_iter().map(|(w, c)| serde_json::json!({ "topic_keyword": w, "message_count": c })).collect::<Vec<_>>(),
        "active_threads": threads,
        "links_shared": links.most_common(10).into_iter().map(|(u, c)| serde_json::json!({ "url": u, "count": c })).collect::<Vec<_>>(),
    })
}

/// Messages with a keyword, or a request ("please x", "can/could you x") longer than 20
/// characters (pure).
pub fn action_items(messages: &[Value], keywords: &[String], channel: &str, names: &HashMap<String, String>) -> Value {
    let requests = [r"(?i)\bplease\s+\w+", r"(?i)\bcan\s+you\s+\w+", r"(?i)\bcould\s+you\s+\w+"].map(|p| regex::Regex::new(p).expect("valid"));
    let items: Vec<Value> = messages
        .iter()
        .filter(|m| {
            let text = str_of(m, "text");
            let lower = text.to_lowercase();
            keywords.iter().any(|k| lower.contains(&k.to_lowercase())) || (text.chars().count() > 20 && requests.iter().any(|r| r.is_match(text)))
        })
        .map(|m| {
            let uid = str_of(m, "user");
            serde_json::json!({ "ts": str_of(m, "ts"), "user": uid, "display_name": name(names, uid), "text": str_of(m, "text"), "thread_ts": str_of(m, "thread_ts"), "channel": channel })
        })
        .collect();
    serde_json::json!({ "action_items": items, "count": items.len() })
}

/// The default action-item keywords.
pub fn default_keywords() -> Vec<String> {
    ["TODO", "action item", "follow up", "follow-up", "need to", "assign"].iter().map(|s| s.to_string()).collect()
}
