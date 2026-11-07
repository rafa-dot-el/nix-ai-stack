//! The read side (the Python mcp_slack tools, same shapes): conversations, messages, search,
//! users, with an in-memory user/channel cache.
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

use crate::client::{Client, SlackError};

pub const USER_TTL: Duration = Duration::from_secs(24 * 3600);
pub const CHANNEL_TTL: Duration = Duration::from_secs(3600);

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

/// A user as the tools show it (pure).
pub fn user(raw: &Value) -> Value {
    let p = raw.get("profile").cloned().unwrap_or(Value::Null);
    let real = raw.get("real_name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| s(&p, "real_name"));
    json!({
        "id": s(raw, "id"), "name": s(raw, "name"), "real_name": real, "display_name": s(&p, "display_name"),
        "email": s(&p, "email"), "title": s(&p, "title"), "is_bot": b(raw, "is_bot"), "deleted": b(raw, "deleted"),
    })
}

/// A channel as the tools show it (pure).
pub fn channel(raw: &Value) -> Value {
    let name = raw.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| s(raw, "id"));
    json!({
        "id": s(raw, "id"), "name": name, "is_private": b(raw, "is_private"), "is_member": b(raw, "is_member"),
        "topic": raw.pointer("/topic/value").and_then(Value::as_str).unwrap_or(""),
        "purpose": raw.pointer("/purpose/value").and_then(Value::as_str).unwrap_or(""),
        "num_members": raw.get("num_members").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// The best name for a user value: display, real, then login name, else the id (pure).
pub fn display_name(user: &Value, id: &str) -> String {
    ["display_name", "real_name", "name"].iter().map(|k| s(user, k)).find(|n| !n.is_empty()).unwrap_or_else(|| id.to_string())
}

fn limit(n: usize) -> String {
    n.clamp(1, 1000).to_string()
}

pub struct Slack {
    pub client: Client,
    users: Mutex<HashMap<String, (Value, Instant)>>,
    channels: Mutex<HashMap<String, (Value, Instant)>>,
    /// (own user id, own self-DM id), once asked.
    own: Mutex<(Option<String>, Option<String>)>,
}

impl Slack {
    pub fn new(client: Client) -> Slack {
        Slack { client, users: Mutex::new(HashMap::new()), channels: Mutex::new(HashMap::new()), own: Mutex::new((None, None)) }
    }

    /// The token's own user id (auth.test, once).
    pub async fn own_id(&self) -> Result<String, SlackError> {
        if let Some(id) = self.own.lock().expect("cache").0.clone() {
            return Ok(id);
        }
        let id = s(&self.client.call("auth.test", &[]).await?, "user_id");
        self.own.lock().expect("cache").0 = Some(id.clone());
        Ok(id)
    }

    /// The DM channel with `user` (conversations.open).
    pub async fn dm_with(&self, user: &str) -> Result<String, SlackError> {
        let r = self.client.call("conversations.open", &[("users", user.to_string())]).await?;
        Ok(r.pointer("/channel/id").and_then(Value::as_str).unwrap_or("").to_string())
    }

    /// The user's own DM (once).
    pub async fn self_dm(&self) -> Result<String, SlackError> {
        if let Some(id) = self.own.lock().expect("cache").1.clone() {
            return Ok(id);
        }
        let own = self.own_id().await?;
        let id = self.dm_with(&own).await?;
        self.own.lock().expect("cache").1 = Some(id.clone());
        Ok(id)
    }

    /// A user id by exact login or display name (case-insensitive), listing users if needed.
    pub async fn user_id_by_name(&self, name: &str) -> Result<Option<String>, SlackError> {
        let lower = name.to_lowercase();
        let lookup = |this: &Slack| {
            this.users.lock().expect("cache").values().map(|(u, _)| u.clone()).find(|u| s(u, "name").to_lowercase() == lower || s(u, "display_name").to_lowercase() == lower).map(|u| s(&u, "id"))
        };
        if let Some(id) = lookup(self) {
            return Ok(Some(id));
        }
        self.users_list(10_000, true).await?;
        Ok(lookup(self))
    }

    /// A channel by name, from the cache or the channel list.
    pub async fn channel_by_name(&self, name: &str) -> Result<Option<Value>, SlackError> {
        let find = |this: &Slack| this.channels.lock().expect("cache").values().map(|(c, _)| c.clone()).find(|c| s(c, "name") == name);
        if let Some(c) = find(self) {
            return Ok(Some(c));
        }
        self.conversations_list("public_channel,private_channel", 10_000).await?;
        Ok(find(self))
    }

    fn remember_user(&self, u: &Value) {
        self.users.lock().expect("cache").insert(s(u, "id"), (u.clone(), Instant::now()));
    }

    fn cached_user(&self, id: &str) -> Option<Value> {
        self.users.lock().expect("cache").get(id).filter(|(_, at)| at.elapsed() < USER_TTL).map(|(u, _)| u.clone())
    }

    fn name_of(&self, id: &str) -> String {
        self.cached_user(id).map_or_else(|| id.to_string(), |u| display_name(&u, id))
    }

    pub fn clear_cache(&self, which: &str) -> Value {
        let mut cleared = Vec::new();
        if matches!(which, "all" | "users") {
            self.users.lock().expect("cache").clear();
            cleared.push("users");
        }
        if matches!(which, "all" | "channels") {
            self.channels.lock().expect("cache").clear();
            cleared.push("channels");
        }
        json!({ "cleared": cleared })
    }

    pub fn cache_stats(&self) -> Value {
        json!({ "users": self.users.lock().expect("cache").len(), "channels": self.channels.lock().expect("cache").len() })
    }

    async fn info_of(&self, channel_id: &str) -> Result<Value, SlackError> {
        let r = self.client.call("conversations.info", &[("channel", channel_id.to_string())]).await?;
        Ok(r.get("channel").cloned().unwrap_or(Value::Null))
    }

    pub async fn conversations_list(&self, types: &str, limit_n: usize) -> Result<Value, SlackError> {
        let form = [("types", types.to_string()), ("limit", limit(limit_n)), ("exclude_archived", "true".to_string())];
        let raw = match self.client.paged("conversations.list", &form, "channels", limit_n.max(1)).await {
            Ok(r) => r,
            Err(SlackError::Api { code, .. }) if code == "enterprise_is_restricted" => {
                let counts = self.client.call("client.counts", &[("org_wide_aware", "true".to_string())]).await?;
                let mut out = Vec::new();
                for id in counts.get("channels").and_then(Value::as_array).into_iter().flatten().filter_map(|c| c.get("id").and_then(Value::as_str)) {
                    if let Ok(info) = self.info_of(id).await {
                        out.push(info);
                    }
                }
                out
            }
            Err(e) => return Err(e),
        };
        let channels: Vec<Value> = raw.iter().map(channel).collect();
        let mut cache = self.channels.lock().expect("cache");
        for c in &channels {
            cache.insert(s(c, "id"), (c.clone(), Instant::now()));
        }
        Ok(json!({ "channels": channels, "count": channels.len() }))
    }

    pub async fn conversations_info(&self, channel_id: &str) -> Result<Value, SlackError> {
        let ch = self.info_of(channel_id).await?;
        Ok(json!({
            "id": s(&ch, "id"), "name": s(&ch, "name"), "is_private": b(&ch, "is_private"), "is_archived": b(&ch, "is_archived"),
            "topic": ch.pointer("/topic/value").and_then(Value::as_str).unwrap_or(""), "purpose": ch.pointer("/purpose/value").and_then(Value::as_str).unwrap_or(""),
            "creator": s(&ch, "creator"), "created": ch.get("created").and_then(Value::as_i64).unwrap_or(0), "num_members": ch.get("num_members").and_then(Value::as_u64).unwrap_or(0),
        }))
    }

    pub async fn conversations_members(&self, channel_id: &str, limit_n: usize) -> Result<Value, SlackError> {
        let ids = self.client.paged("conversations.members", &[("channel", channel_id.to_string()), ("limit", limit(limit_n))], "members", limit_n.max(1)).await?;
        let members: Vec<Value> = ids.iter().filter_map(Value::as_str).map(|id| json!({ "user_id": id, "display_name": self.name_of(id) })).collect();
        Ok(json!({ "members": members, "count": members.len() }))
    }

    fn message(&self, m: &Value) -> Value {
        let uid = m.get("user").or_else(|| m.get("bot_id")).and_then(Value::as_str).unwrap_or("unknown").to_string();
        json!({
            "user": uid, "display_name": self.name_of(&uid), "text": s(m, "text"), "ts": s(m, "ts"), "thread_ts": s(m, "thread_ts"),
            "reply_count": m.get("reply_count").and_then(Value::as_u64).unwrap_or(0),
            "reactions": m.get("reactions").and_then(Value::as_array).into_iter().flatten().map(|r| json!({ "name": r.get("name"), "count": r.get("count") })).collect::<Vec<_>>(),
            "subtype": s(m, "subtype"),
        })
    }

    pub async fn history(&self, channel_id: &str, limit_n: usize, oldest: Option<&str>, latest: Option<&str>) -> Result<Value, SlackError> {
        let mut form = vec![("channel", channel_id.to_string()), ("limit", limit(limit_n))];
        if let Some(o) = oldest {
            form.push(("oldest", o.to_string()));
        }
        if let Some(l) = latest {
            form.push(("latest", l.to_string()));
        }
        let r = self.client.call("conversations.history", &form).await?;
        let messages: Vec<Value> = r.get("messages").and_then(Value::as_array).into_iter().flatten().map(|m| self.message(m)).collect();
        Ok(json!({ "messages": messages, "count": messages.len(), "has_more": b(&r, "has_more") }))
    }

    pub async fn replies(&self, channel_id: &str, ts: &str, limit_n: usize) -> Result<Value, SlackError> {
        let raw = self.client.paged("conversations.replies", &[("channel", channel_id.to_string()), ("ts", ts.to_string()), ("limit", limit(limit_n))], "messages", limit_n.max(1)).await?;
        let messages: Vec<Value> = raw.iter().map(|m| self.message(m)).collect();
        Ok(json!({ "messages": messages, "count": messages.len() }))
    }

    pub async fn search(&self, query: &str, count: usize, page: usize) -> Result<Value, SlackError> {
        let form = [("query", query.to_string()), ("sort", "timestamp".to_string()), ("sort_dir", "desc".to_string()), ("count", count.clamp(1, 100).to_string()), ("page", page.max(1).to_string())];
        let r = self.client.call("search.messages", &form).await?;
        let m = r.get("messages").cloned().unwrap_or(Value::Null);
        let matches: Vec<Value> = m
            .get("matches")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|x| {
                let user = x.get("user").or_else(|| x.get("username")).and_then(Value::as_str).unwrap_or("unknown");
                json!({ "user": user, "text": s(x, "text"), "ts": s(x, "ts"), "channel": x.pointer("/channel/name").and_then(Value::as_str).unwrap_or(""),
                    "channel_id": x.pointer("/channel/id").and_then(Value::as_str).unwrap_or(""), "permalink": s(x, "permalink") })
            })
            .collect();
        Ok(json!({ "matches": matches, "count": matches.len(), "total": m.get("total").cloned().unwrap_or(json!(0)),
            "page": m.pointer("/paging/page").cloned().unwrap_or(json!(page)), "pages": m.pointer("/paging/pages").cloned().unwrap_or(json!(1)) }))
    }

    pub async fn users_list(&self, limit_n: usize, include_bots: bool) -> Result<Value, SlackError> {
        let raw = self.client.paged("users.list", &[("limit", limit(limit_n))], "members", limit_n.max(1)).await?;
        let users: Vec<Value> = raw.iter().map(user).filter(|u| include_bots || !b(u, "is_bot")).collect();
        for u in &users {
            self.remember_user(u);
        }
        Ok(json!({ "users": users, "count": users.len() }))
    }

    pub async fn users_info(&self, id: &str) -> Result<Value, SlackError> {
        if let Some(u) = self.cached_user(id) {
            return Ok(u);
        }
        let r = self.client.call("users.info", &[("user", id.to_string())]).await?;
        let u = user(r.get("user").unwrap_or(&Value::Null));
        self.remember_user(&u);
        Ok(u)
    }

    pub async fn find_user(&self, query: &str) -> Result<Value, SlackError> {
        let q = query.trim();
        let is_id = q.len() > 1 && q.starts_with(['U', 'W']) && q[1..].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if is_id {
            return Ok(json!({ "users": [self.users_info(q).await?] }));
        }
        let lower = q.to_lowercase();
        let cached: Vec<Value> = self.users.lock().expect("cache").values().map(|(u, _)| u.clone()).collect();
        if q.contains('@') {
            if let Some(u) = cached.iter().find(|u| s(u, "email").to_lowercase() == lower) {
                return Ok(json!({ "users": [u] }));
            }
            return match self.client.call("users.lookupByEmail", &[("email", q.to_string())]).await {
                Ok(r) => {
                    let u = user(r.get("user").unwrap_or(&Value::Null));
                    self.remember_user(&u);
                    Ok(json!({ "users": [u] }))
                }
                Err(SlackError::Api { .. }) => Ok(json!({ "users": [], "error": format!("no user with email {q}") })),
                Err(e) => Err(e),
            };
        }
        let mut found: Vec<Value> = cached.into_iter().filter(|u| ["name", "display_name", "real_name"].iter().any(|k| s(u, k).to_lowercase().contains(&lower))).collect();
        found.sort_by_key(|u| s(u, "id"));
        if found.is_empty() {
            return Ok(json!({ "users": [], "error": format!("no user matching {q:?} (users_list fills the cache)") }));
        }
        Ok(json!({ "users": found }))
    }

    /// The user's channels in one paged users.conversations (fix: the old code checked the
    /// first 100 members of every channel, missing larger ones).
    pub async fn find_user_channels(&self, user_id: &str, types: &str) -> Result<Value, SlackError> {
        let raw = self.client.paged("users.conversations", &[("user", user_id.to_string()), ("types", types.to_string()), ("limit", "200".to_string())], "channels", 10_000).await?;
        let channels: Vec<Value> = raw.iter().map(|c| json!({ "channel_id": s(c, "id"), "name": s(c, "name"), "is_private": b(c, "is_private") })).collect();
        Ok(json!({ "channels": channels, "count": channels.len() }))
    }

    pub async fn resolve_user_ids(&self, ids: &[String]) -> Result<Value, SlackError> {
        let mut out = serde_json::Map::new();
        for id in ids {
            let name = match self.users_info(id).await {
                Ok(u) => display_name(&u, id),
                Err(_) => id.clone(),
            };
            out.insert(id.clone(), name.into());
        }
        Ok(Value::Object(out))
    }
}

impl Slack {
    /// Raw messages of `channel` since `oldest` (paged, at most `max`).
    pub async fn raw_history(&self, channel_id: &str, oldest: Option<String>, max: usize) -> Result<Vec<Value>, SlackError> {
        let mut form = vec![("channel", channel_id.to_string()), ("limit", "200".to_string())];
        if let Some(o) = oldest {
            form.push(("oldest", o));
        }
        self.client.paged("conversations.history", &form, "messages", max).await
    }

    /// Display names for every user id in `messages` (cache, then users.info).
    pub async fn names_for(&self, messages: &[Value]) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for id in messages.iter().filter_map(|m| m.get("user").and_then(Value::as_str)) {
            if !out.contains_key(id) {
                let name = self.users_info(id).await.map(|u| display_name(&u, id)).unwrap_or_else(|_| id.to_string());
                out.insert(id.to_string(), name);
            }
        }
        out
    }

    /// The Python's dump_recent_messages: last N messages, or the last months, in chunks.
    pub async fn dump(&self, channel_id: &str, mode: &str, count: usize, months: u64, chunk: usize) -> Result<Value, SlackError> {
        let oldest = (mode == "time_range").then(|| (unix_now() - months as f64 * 30.0 * 86_400.0).to_string());
        let max = if mode == "last_n" { count } else { 100_000 };
        let raw = self.raw_history(channel_id, oldest, max).await?;
        let messages: Vec<Value> = raw.iter().map(|m| self.message(m)).collect();
        let chunks: Vec<Value> = messages.chunks(chunk.max(1)).enumerate().map(|(i, c)| json!({ "chunk_index": i, "messages": c, "count": c.len() })).collect();
        Ok(json!({ "channel": channel_id, "total_messages": messages.len(), "chunks": chunks }))
    }

    /// The Python's my_interactions: recent DMs and the channels mentioning the user.
    pub async fn my_interactions(&self, days: u64, limit_n: usize) -> Result<Value, SlackError> {
        let own = self.own_id().await?;
        let oldest = (unix_now() - days as f64 * 86_400.0).to_string();
        let ims = self.client.call("conversations.list", &[("types", "im".to_string()), ("limit", "200".to_string())]).await?;
        let mut dms = Vec::new();
        for im in ims.get("channels").and_then(Value::as_array).into_iter().flatten().take(limit_n) {
            let hist = self.client.call("conversations.history", &[("channel", s(im, "id")), ("limit", "10".to_string()), ("oldest", oldest.clone())]).await?;
            let msgs = hist.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(first) = msgs.first() {
                let other = s(im, "user");
                dms.push(json!({ "user": other, "display_name": self.name_of(&other), "message_count": msgs.len(), "last_message_ts": s(first, "ts") }));
            }
        }
        dms.sort_by_key(|d| std::cmp::Reverse(s(d, "last_message_ts")));
        let found = self.client.call("search.messages", &[("query", format!("<@{own}>")), ("count", "50".to_string())]).await?;
        let mut mentions: Vec<(String, u64)> = Vec::new();
        for m in found.pointer("/messages/matches").and_then(Value::as_array).into_iter().flatten() {
            let name = m.pointer("/channel/name").and_then(Value::as_str).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            match mentions.iter_mut().find(|(n, _)| *n == name) {
                Some((_, c)) => *c += 1,
                None => mentions.push((name, 1)),
            }
        }
        mentions.sort_by_key(|m| std::cmp::Reverse(m.1));
        let mentioned: Vec<Value> = mentions.into_iter().take(limit_n).map(|(c, n)| json!({ "channel": c, "mention_count": n })).collect();
        dms.truncate(limit_n);
        Ok(json!({ "dm_conversations": dms, "mentioned_in": mentioned }))
    }
}

/// Seconds since the epoch, as Slack's oldest/latest want them.
pub fn unix_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
