//! A message to send, as a file (`--from-yaml`, `--from-json`) or the `slack_send` tool's
//! arguments: one schema (plan 5.2). Validation refuses unsafe combinations (pure).
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attach {
    pub path: String,
    /// image | snippet | file (default)
    #[serde(default, rename = "as")]
    pub kind: Option<String>,
    pub alt: Option<String>,
    pub title: Option<String>,
    /// Snippet language.
    pub lang: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub csv: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unfurl {
    pub links: Option<bool>,
    pub media: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    /// RFC 3339, or local `YYYY-MM-DD HH:MM` in `tz`.
    pub at: String,
    pub tz: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub to: String,
    pub title: Option<String>,
    pub text: Option<String>,
    pub markdown: Option<String>,
    pub markdown_file: Option<String>,
    /// Raw Block Kit.
    pub blocks: Option<serde_json::Value>,
    #[serde(default)]
    pub tables: Vec<Table>,
    #[serde(default)]
    pub attach: Vec<Attach>,
    /// Text for the shared voice (TTS), attached as audio.
    pub say: Option<String>,
    #[serde(default)]
    pub unfurl: Unfurl,
    /// A permalink or a ts.
    pub thread: Option<String>,
    #[serde(default)]
    pub broadcast: bool,
    pub schedule: Option<Schedule>,
    /// live | draft (still subject to the policy).
    pub mode: Option<String>,
}

pub fn from_yaml(text: &str) -> Result<Spec, String> {
    serde_yaml_ng::from_str(text).map_err(|e| format!("message YAML: {e}"))
}

pub fn from_json(text: &str) -> Result<Spec, String> {
    serde_json::from_str(text).map_err(|e| format!("message JSON: {e}"))
}

/// The combinations Slack (or the policy) cannot honour, refused before anything is sent.
pub fn validate(spec: &Spec) -> Result<(), String> {
    let bodies = [spec.text.is_some(), spec.markdown.is_some(), spec.markdown_file.is_some(), spec.blocks.is_some()].iter().filter(|b| **b).count();
    if bodies > 1 {
        return Err("give one of text, markdown, markdown_file or blocks".into());
    }
    let files = !spec.attach.is_empty() || spec.say.is_some();
    if bodies == 0 && spec.tables.is_empty() && !files {
        return Err("nothing to send: no text, markdown, blocks, tables, attachments or say".into());
    }
    if spec.schedule.is_some() && files {
        return Err("scheduled messages cannot carry files (attach or say); send the files first and reference them".into());
    }
    if spec.broadcast && spec.thread.is_none() {
        return Err("broadcast only applies to a reply: give a thread".into());
    }
    if let Some(mode) = spec.mode.as_deref().filter(|m| !matches!(*m, "live" | "draft")) {
        return Err(format!("mode must be live or draft, not {mode:?}"));
    }
    if let Some(a) = spec.attach.iter().find(|a| !matches!(a.kind.as_deref(), None | Some("image" | "snippet" | "file"))) {
        return Err(format!("attachment {:?}: as must be image, snippet or file, not {:?}", a.path, a.kind.as_deref().unwrap_or("")));
    }
    Ok(())
}
