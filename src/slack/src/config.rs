//! ~/.config/rafa/slack.yaml (plan 5.4): credentials as secret refs, defaults, policy,
//! attachment roots and limits, tables, TTS.
use rafa_config::{SecretRef, Value};

use crate::policy::{Action, Match, Policy, Rule};

/// Slack's own per-file maximum (`max_bytes: slack`). Secondary sources say 1 GiB; Slack's
/// files docs do not state it, so it is one constant to check against them.
pub const SLACK_MAX_FILE_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Mrkdwn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    Split,
    Canvas,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub token: Option<SecretRef>,
    pub cookie: Option<SecretRef>,
    pub format: Format,
    pub unfurl_links: bool,
    pub unfurl_media: bool,
    pub timezone: Option<String>,
    pub policy: Policy,
    pub allowed_roots: Vec<String>,
    pub max_bytes: u64,
    pub max_rows_per_message: usize,
    pub overflow: Overflow,
    pub tts: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("{path}: {reason}")]
    Invalid { path: String, reason: String },
    #[error(transparent)]
    Secret(#[from] rafa_config::ConfigError),
}

/// The typed view; `None` (no file): defaults, and a policy that denies everything.
pub fn from_value(value: Option<&Value>) -> Result<Config, ConfigError> {
    let invalid = |path: &str, reason: &str| ConfigError::Invalid { path: path.to_string(), reason: reason.to_string() };
    let get = |path: &str| -> Option<&Value> { path.split('.').try_fold(value?, |v, k| v.get(k)) };
    let str_at = |path: &str| -> Result<Option<String>, ConfigError> {
        match get(path) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(invalid(path, "expected a string")),
        }
    };
    let bool_at = |path: &str, default: bool| -> Result<bool, ConfigError> {
        match get(path) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(invalid(path, "expected true or false")),
        }
    };
    let secret = |path: &str| -> Result<Option<SecretRef>, ConfigError> {
        match get(path) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => Ok(Some(SecretRef::parse(v, path)?)),
        }
    };
    let action = |path: &str, v: &str| match v {
        "live" => Ok(Action::Live),
        "draft" => Ok(Action::Draft),
        "deny" => Ok(Action::Deny),
        _ => Err(invalid(path, "expected live, draft or deny")),
    };
    let format = match str_at("defaults.format")?.as_deref() {
        None | Some("markdown") => Format::Markdown,
        Some("mrkdwn") => Format::Mrkdwn,
        Some(_) => return Err(invalid("defaults.format", "expected markdown or mrkdwn")),
    };
    let default = match str_at("policy.default")? {
        None => Action::Deny,
        Some(v) => action("policy.default", &v)?,
    };
    let mut rules = Vec::new();
    if let Some(list) = get("policy.rules") {
        let list = list.as_sequence().ok_or_else(|| invalid("policy.rules", "expected a list"))?;
        for (i, rule) in list.iter().enumerate() {
            let path = format!("policy.rules[{i}]");
            let m = rule.get("match").and_then(Value::as_mapping).ok_or_else(|| invalid(&path, "needs match: {...}"))?;
            let mut matches = Match::default();
            for (k, v) in m {
                match (k.as_str(), v) {
                    (Some("self_dm"), Value::Bool(b)) => matches.self_dm = Some(*b),
                    (Some("channel"), Value::String(g)) => {
                        globset::Glob::new(g).map_err(|e| invalid(&path, &format!("bad channel glob: {e}")))?;
                        matches.channel = Some(g.clone());
                    }
                    (Some("channel_id"), Value::String(id)) => matches.channel_id = Some(id.clone()),
                    _ => return Err(invalid(&path, "match keys are self_dm, channel and channel_id")),
                }
            }
            let a = rule.get("action").and_then(Value::as_str).ok_or_else(|| invalid(&path, "needs action"))?;
            rules.push(Rule { matches, action: action(&path, a)? });
        }
    }
    let allowed_roots = match get("attachments.allowed_roots") {
        None => Vec::new(),
        Some(v) => v
            .as_sequence()
            .and_then(|s| s.iter().map(|x| x.as_str().map(str::to_string)).collect::<Option<Vec<_>>>())
            .ok_or_else(|| invalid("attachments.allowed_roots", "expected a list of paths"))?,
    };
    let max_bytes = match get("attachments.max_bytes") {
        None | Some(Value::Null) => SLACK_MAX_FILE_BYTES,
        Some(Value::String(s)) if s == "slack" => SLACK_MAX_FILE_BYTES,
        Some(Value::Number(n)) => match n.as_u64() {
            Some(b) if b <= SLACK_MAX_FILE_BYTES => b,
            Some(_) => return Err(invalid("attachments.max_bytes", "above Slack's own per-file maximum")),
            None => return Err(invalid("attachments.max_bytes", "expected a positive number")),
        },
        Some(_) => return Err(invalid("attachments.max_bytes", "expected slack or a number of bytes")),
    };
    let max_rows_per_message = match get("tables.max_rows_per_message") {
        None | Some(Value::Null) => 100,
        Some(v) => v.as_u64().filter(|n| (1..=100).contains(n)).ok_or_else(|| invalid("tables.max_rows_per_message", "expected 1 to 100"))? as usize,
    };
    let overflow = match str_at("tables.overflow")?.as_deref() {
        None | Some("canvas") => Overflow::Canvas,
        Some("split") => Overflow::Split,
        Some("file") => Overflow::File,
        Some(_) => return Err(invalid("tables.overflow", "expected canvas, split or file")),
    };
    Ok(Config {
        token: secret("credentials.token")?,
        cookie: secret("credentials.cookie")?,
        format,
        unfurl_links: bool_at("defaults.unfurl.links", false)?,
        unfurl_media: bool_at("defaults.unfurl.media", true)?,
        timezone: str_at("defaults.timezone")?,
        policy: Policy { default, rules },
        allowed_roots,
        max_bytes,
        max_rows_per_message,
        overflow,
        tts: bool_at("tts.enable", true)?,
    })
}
