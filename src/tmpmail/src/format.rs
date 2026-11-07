//! Output formatting: every command can render its result as `text` (an
//! 80-column human-readable view), `json`, or `yaml`. The view structs here
//! are the serialization boundary — they never expose `sid_token`.

use std::fmt;
use std::str::FromStr;

use serde::Serialize;

use crate::identities::Identity;

pub const TEXT_WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Yaml,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "text" | "txt" | "plain" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "yaml" | "yml" => Ok(OutputFormat::Yaml),
            other => Err(format!(
                "unknown format '{other}' (expected 'text', 'json', or 'yaml')"
            )),
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Yaml => write!(f, "yaml"),
        }
    }
}

/// Render `value` as JSON/YAML directly, or via `text_view()` for the text format.
pub fn render<T: Serialize>(
    format: OutputFormat,
    value: &T,
    text_view: impl FnOnce() -> String,
) -> Result<String, String> {
    match format {
        OutputFormat::Text => Ok(text_view()),
        OutputFormat::Json => serde_json::to_string_pretty(value).map(|s| s + "\n").map_err(|e| format!("JSON output: {e}")),
        OutputFormat::Yaml => serde_yaml_ng::to_string(value).map_err(|e| format!("YAML output: {e}")),
    }
}

/// Greedy word-wrap to `width` columns. Preserves existing line breaks
/// (each input line is wrapped independently); does not hyphenate.
pub fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let lines = text.split('\n');
    for paragraph in lines {
        if paragraph.is_empty() {
            out.push('\n');
            continue;
        }
        let mut line_len = 0usize;
        for (i, word) in paragraph.split_whitespace().enumerate() {
            if i > 0 && line_len + 1 + word.chars().count() > width {
                out.push('\n');
                line_len = 0;
            } else if i > 0 {
                out.push(' ');
                line_len += 1;
            }
            out.push_str(word);
            line_len += word.chars().count();
        }
        out.push('\n');
    }
    out
}

/// Truncate to `width` display columns, appending an ellipsis if cut.
pub fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

#[derive(Debug, Serialize)]
pub struct IdentityView {
    pub email: String,
    pub created_at: u64,
}

impl From<&Identity> for IdentityView {
    fn from(identity: &Identity) -> Self {
        Self {
            email: identity.email.clone(),
            created_at: identity.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MessageView {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub read: bool,
    pub excerpt: String,
}

#[derive(Debug, Serialize)]
pub struct MessageDetailView {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub body: String,
}

pub fn identity_list_text(identities: &[IdentityView]) -> String {
    if identities.is_empty() {
        return "(no identities configured — run `tmpmail new-identity`)\n".to_string();
    }
    let mut out = String::new();
    for identity in identities {
        out.push_str(&format!(
            "{:<50} created_at={}\n",
            truncate(&identity.email, 50),
            identity.created_at
        ));
    }
    out
}

pub fn identity_created_text(identity: &IdentityView) -> String {
    format!("{}\n", identity.email)
}

pub fn message_list_text(messages: &[MessageView]) -> String {
    if messages.is_empty() {
        return "(inbox empty)\n".to_string();
    }
    let mut out = String::new();
    for msg in messages {
        let marker = if msg.read { "R" } else { "N" };
        out.push_str(&truncate(
            &format!(
                "[{}] {} {}  from={}  subject={:?}",
                msg.id, marker, msg.date, msg.from, msg.subject
            ),
            TEXT_WIDTH,
        ));
        out.push('\n');
        if !msg.excerpt.trim().is_empty() {
            let first_line = msg.excerpt.lines().next().unwrap_or("");
            out.push_str("      ");
            out.push_str(&truncate(first_line, TEXT_WIDTH - 6));
            out.push('\n');
        }
    }
    out
}

/// Extremely small HTML-to-text stripper — good enough for GuerrillaMail's
/// simple message bodies, not a general-purpose HTML parser.
pub fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_html_entities(&out)
}

fn decode_html_entities(input: &str) -> String {
    // &amp; last: decoding it first would turn the quoted text "&amp;lt;" into "<".
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&laquo;", "\u{ab}")
        .replace("&raquo;", "\u{bb}")
        .replace("&amp;", "&")
}

pub fn message_detail_text(msg: &MessageDetailView, body: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("Id:      {}\n", msg.id));
    out.push_str(&format!("From:    {}\n", msg.from));
    out.push_str(&format!("Subject: {}\n", msg.subject));
    out.push_str(&format!("Date:    {}\n", msg.date));
    out.push('\n');
    out.push_str(&wrap(body, TEXT_WIDTH));
    out
}

