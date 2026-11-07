//! Where a message goes, as the user writes it (pure).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `me` / `myself`: the user's own DM.
    Me,
    /// `@name` or `U…`/`W…`: a DM with that user.
    User(String),
    /// `#name` or a bare name: a channel by name.
    ChannelName(String),
    /// `C…`, `G…`, `D…`: a conversation id.
    ChannelId(String),
}

/// A message permalink: `https://<ws>.slack.com/archives/<C>/p<16 digits>[?thread_ts=…]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permalink {
    pub channel: String,
    pub ts: String,
    /// The thread it belongs to, when the link is to a reply.
    pub thread_ts: Option<String>,
}

pub fn parse(target: &str) -> Result<Target, String> {
    let t = target.trim();
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    let id = |s: &str, kinds: &str| s.len() >= 2 && s.starts_with(|c: char| kinds.contains(c)) && s[1..].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if t.eq_ignore_ascii_case("me") || t.eq_ignore_ascii_case("myself") {
        return Ok(Target::Me);
    }
    if let Some(user) = t.strip_prefix('@') {
        return if word(user) { Ok(Target::User(user.to_string())) } else { Err(format!("{target:?} is not a user (@name)")) };
    }
    if id(t, "UW") {
        return Ok(Target::User(t.to_string()));
    }
    if id(t, "CGD") {
        return Ok(Target::ChannelId(t.to_string()));
    }
    let name = t.strip_prefix('#').unwrap_or(t);
    if word(name) { Ok(Target::ChannelName(name.to_string())) } else { Err(format!("{target:?} is not a target (me, @user, #channel, or an id)")) }
}

pub fn permalink(url: &str) -> Result<Permalink, String> {
    let bad = || format!("{url:?} is not a Slack message link (https://<workspace>.slack.com/archives/<channel>/p<timestamp>)");
    let rest = url.trim().strip_prefix("https://").ok_or_else(bad)?;
    let (host, path) = rest.split_once('/').ok_or_else(bad)?;
    if !host.ends_with(".slack.com") || host.contains(['@', ':']) {
        return Err(bad());
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let mut parts = path.split('/');
    let (Some("archives"), Some(channel), Some(p), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return Err(bad()) };
    let ts = ts_from_p(p).ok_or_else(bad)?;
    if !matches!(parse(channel), Ok(Target::ChannelId(_))) {
        return Err(bad());
    }
    let thread_ts = query.split('&').find_map(|kv| kv.strip_prefix("thread_ts=")).filter(|v| v.len() == 17 && v.as_bytes()[10] == b'.').map(str::to_string);
    Ok(Permalink { channel: channel.to_string(), ts, thread_ts })
}

/// `p1712345678123456` -> `1712345678.123456`.
pub fn ts_from_p(p: &str) -> Option<String> {
    let digits = p.strip_prefix('p')?;
    (digits.len() == 16 && digits.chars().all(|c| c.is_ascii_digit())).then(|| format!("{}.{}", &digits[..10], &digits[10..]))
}
