//! Sending (plan 5.2/5.3): a pure plan of the requests, then their execution. The plan is
//! what --dry-run --print-payload shows; the policy decided live, draft or deny before it.
use serde_json::Value;

use crate::{
    blocks::{markdown_messages, table_messages},
    config::Config,
    policy::Action,
    wire::{draft_form, post_message, Post},
};

/// A file to upload, already checked against the attachment roots and the size limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub title: Option<String>,
    pub alt: Option<String>,
    pub snippet_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Upload; `share`: complete it into the channel (live), else privately (drafts).
    Upload { file: File, share: bool },
    Post(Value),
    Schedule(Value),
    /// drafts.create form fields; the executor sets file_ids from the uploads before it.
    Draft(Vec<(String, String)>),
}

/// The resolved message: markdown text (loaded from markdown_file already), plain text,
/// raw blocks, CSV tables (parsed), files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Body {
    pub markdown: Option<String>,
    pub text: Option<String>,
    pub blocks: Option<Value>,
    pub tables: Vec<Vec<Vec<String>>>,
    pub files: Vec<File>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub thread_ts: Option<String>,
    pub broadcast: bool,
    pub unfurl_links: Option<bool>,
    pub unfurl_media: Option<bool>,
    /// Unix seconds.
    pub post_at: Option<i64>,
    pub client_msg_id: String,
}

/// Is `path` (already canonical) inside one of `roots` (canonical), and small enough? (pure)
pub fn check_file(path: &std::path::Path, size: u64, roots: &[std::path::PathBuf], max_bytes: u64) -> Result<(), String> {
    if roots.is_empty() {
        return Err("no attachments.allowed_roots in slack.yaml: nothing may be attached".into());
    }
    if !roots.iter().any(|r| path.starts_with(r)) {
        return Err(format!("{} is outside attachments.allowed_roots", path.display()));
    }
    if size > max_bytes {
        return Err(format!("{} is {size} bytes, over the limit of {max_bytes}", path.display()));
    }
    Ok(())
}

/// One message's content: exactly what chat.postMessage / drafts.create get.
struct Message {
    markdown: Option<String>,
    text: Option<String>,
    blocks: Option<Value>,
}

fn messages(body: &Body, cfg: &Config) -> Result<Vec<Message>, String> {
    let mut out = Vec::new();
    if let Some(b) = &body.blocks {
        out.push(Message { markdown: None, text: body.text.clone(), blocks: Some(b.clone()) });
    } else if let Some(md) = &body.markdown {
        let parts = markdown_messages(md);
        if parts.len() == 1 {
            out.push(Message { markdown: Some(md.clone()), text: None, blocks: None });
        } else {
            let n = parts.len();
            out.extend(parts.into_iter().enumerate().map(|(i, block)| Message { markdown: None, text: Some(format!("message part {}/{n}", i + 1)), blocks: Some(Value::Array(vec![block])) }));
        }
    } else if let Some(text) = &body.text {
        out.push(Message { markdown: None, text: Some(text.clone()), blocks: None });
    }
    for rows in &body.tables {
        for block in table_messages(rows, cfg.max_rows_per_message)? {
            out.push(Message { markdown: None, text: Some("table".into()), blocks: Some(Value::Array(vec![block])) });
        }
    }
    Ok(out)
}

/// The requests for one message to `channel` under `action` (pure).
pub fn plan(body: &Body, channel: &str, action: Action, opts: &Options, cfg: &Config, session_token: bool) -> Result<Vec<Step>, String> {
    match action {
        Action::Deny => return Err("denied by the slack.yaml policy for this destination".into()),
        Action::Draft if opts.post_at.is_some() => return Err("drafts cannot be scheduled".into()),
        Action::Draft if !session_token => {
            return Err("drafts need a session (xoxc) token and its cookie; refusing to fall back to a live post".into())
        }
        _ => {}
    }
    if opts.post_at.is_some() && !body.files.is_empty() {
        return Err("scheduled messages cannot carry files".into());
    }
    let msgs = messages(body, cfg)?;
    let unfurl_links = opts.unfurl_links.unwrap_or(cfg.unfurl_links);
    let unfurl_media = opts.unfurl_media.unwrap_or(cfg.unfurl_media);
    let mut steps: Vec<Step> = body.files.iter().map(|f| Step::Upload { file: f.clone(), share: action == Action::Live }).collect();
    if action == Action::Draft {
        if msgs.len() > 1 {
            return Err(format!("this message is {} messages long; a draft holds one message", msgs.len()));
        }
        let m = msgs.first();
        let form = draft_form(
            channel,
            &opts.client_msg_id,
            m.and_then(|m| m.text.as_deref()),
            m.and_then(|m| m.markdown.as_deref()),
            m.and_then(|m| m.blocks.as_ref()),
            opts.thread_ts.as_deref(),
            &[],
        );
        steps.push(Step::Draft(form));
        return Ok(steps);
    }
    for m in &msgs {
        let mut v = post_message(&Post {
            channel,
            markdown: m.markdown.as_deref(),
            text: m.text.as_deref(),
            blocks: m.blocks.as_ref(),
            thread_ts: opts.thread_ts.as_deref(),
            broadcast: opts.broadcast,
            unfurl_links,
            unfurl_media,
        });
        steps.push(match opts.post_at {
            Some(at) => {
                v["post_at"] = at.into();
                Step::Schedule(v)
            }
            None => Step::Post(v),
        });
    }
    Ok(steps)
}

/// Where a target goes (effects: auth.test, conversations.open, the channel list).
pub async fn resolve(slack: &crate::read::Slack, target: &crate::target::Target) -> Result<crate::policy::Destination, String> {
    use crate::{policy::Destination, target::Target};
    let e = |err: crate::client::SlackError| err.to_string();
    match target {
        Target::Me => Ok(Destination { channel_id: slack.self_dm().await.map_err(e)?, name: None, self_dm: true }),
        Target::User(who) => {
            let is_id = who.starts_with(['U', 'W']) && who[1..].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
            let id = if is_id { who.clone() } else { slack.user_id_by_name(who).await.map_err(e)?.ok_or_else(|| format!("no user named {who:?}"))? };
            let own = slack.own_id().await.map_err(e)?;
            Ok(Destination { channel_id: slack.dm_with(&id).await.map_err(e)?, name: None, self_dm: id == own })
        }
        Target::ChannelName(name) => {
            let c = slack.channel_by_name(name).await.map_err(e)?.ok_or_else(|| format!("no channel named #{name} (or not a member)"))?;
            Ok(Destination { channel_id: c["id"].as_str().unwrap_or("").to_string(), name: Some(name.clone()), self_dm: false })
        }
        Target::ChannelId(id) if id.starts_with('D') => Ok(Destination { channel_id: id.clone(), name: None, self_dm: *id == slack.self_dm().await.map_err(e)? }),
        Target::ChannelId(id) => {
            let info = slack.conversations_info(id).await.map_err(e)?;
            let name = info["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string);
            Ok(Destination { channel_id: id.clone(), name, self_dm: false })
        }
    }
}

/// Run the steps; returns what was created (ts, draft id, file ids, scheduled id).
pub async fn execute(slack: &crate::read::Slack, steps: Vec<Step>, channel: &str, thread_ts: Option<&str>) -> Result<Value, String> {
    use crate::wire::{complete_upload, upload_url_form};
    let e = |err: crate::client::SlackError| err.to_string();
    let (mut file_ids, mut ts, mut scheduled) = (Vec::<String>::new(), Vec::<Value>::new(), Vec::<Value>::new());
    let mut draft_id = Value::Null;
    for step in steps {
        match step {
            Step::Upload { file, share } => {
                let form = upload_url_form(&file.name, file.size, file.alt.as_deref(), file.snippet_type.as_deref());
                let refs: Vec<(&str, String)> = form.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
                let r = slack.client.call("files.getUploadURLExternal", &refs).await.map_err(e)?;
                let (url, id) = (r["upload_url"].as_str().unwrap_or(""), r["file_id"].as_str().unwrap_or("").to_string());
                let bytes = std::fs::read(&file.path).map_err(|err| format!("upload {}: {err}", file.path))?;
                slack.client.upload(url, bytes).await.map_err(|err| format!("upload {}: {err}", file.name))?;
                let title = file.title.clone().unwrap_or_else(|| file.name.clone());
                let done = complete_upload(&[(id.clone(), title)], share.then_some(channel), if share { thread_ts } else { None });
                slack.client.call_json("files.completeUploadExternal", &done).await.map_err(e)?;
                file_ids.push(id);
            }
            Step::Draft(mut form) => {
                if let Some(f) = form.iter_mut().find(|(k, _)| k == "file_ids") {
                    f.1 = serde_json::json!(file_ids).to_string();
                }
                let refs: Vec<(&str, String)> = form.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
                let r = slack.client.call("drafts.create", &refs).await.map_err(e)?;
                draft_id = r.pointer("/draft/id").cloned().unwrap_or(Value::Null);
            }
            Step::Post(body) => ts.push(slack.client.call_json("chat.postMessage", &body).await.map_err(e)?["ts"].clone()),
            Step::Schedule(body) => scheduled.push(slack.client.call_json("chat.scheduleMessage", &body).await.map_err(e)?["scheduled_message_id"].clone()),
        }
    }
    Ok(serde_json::json!({ "channel": channel, "file_ids": file_ids, "ts": ts, "draft_id": draft_id, "scheduled_message_ids": scheduled }))
}
