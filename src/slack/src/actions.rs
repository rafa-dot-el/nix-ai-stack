//! What the CLI and the MCP tools do, through one policy: send a spec (or plan it: a dry
//! run), and the other writes (edit, delete, react, ephemeral), which need a live verdict.
use serde_json::Value;

use crate::{
    config::Config,
    policy::{allowed, decide, Action},
    prepare::prepare,
    read::Slack,
    send::{execute, plan, resolve, Step},
    spec::Spec,
    target::{parse, permalink, Target},
};

fn requests(steps: &[Step]) -> Value {
    Value::Array(
        steps
            .iter()
            .map(|s| match s {
                Step::Upload { file, share } => serde_json::json!({ "upload": { "name": file.name, "size": file.size, "shared_into_channel": share } }),
                Step::Post(v) => serde_json::json!({ "chat.postMessage": v }),
                Step::Schedule(v) => serde_json::json!({ "chat.scheduleMessage": v }),
                Step::Draft(form) => serde_json::json!({ "drafts.create": form.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<serde_json::Map<String, Value>>() }),
            })
            .collect(),
    )
}

fn mode(m: Option<&str>) -> Option<Action> {
    match m {
        Some("live") => Some(Action::Live),
        Some("draft") => Some(Action::Draft),
        _ => None,
    }
}

/// Resolve, decide, plan; then execute unless `dry_run` (which returns the decision and the
/// exact requests instead).
pub async fn send_spec(slack: &Slack, cfg: &Config, spec: &Spec, dry_run: bool, tts: &[String], client_msg_id: &str) -> Result<Value, String> {
    let (body, opts) = prepare(spec, cfg, tts, client_msg_id)?;
    let target = parse(&spec.to)?;
    let dest = resolve(slack, &target).await?;
    let (verdict, rule) = decide(&cfg.policy, &dest);
    let action = allowed(verdict, mode(spec.mode.as_deref()))?;
    let steps = plan(&body, &dest.channel_id, action, &opts, cfg, slack.client.is_session())?;
    if dry_run {
        return Ok(serde_json::json!({ "action": action, "rule": rule, "channel": dest.channel_id, "requests": requests(&steps) }));
    }
    let mut done = execute(slack, steps, &dest.channel_id, opts.thread_ts.as_deref()).await?;
    done["action"] = serde_json::to_value(action).expect("serialisable");
    Ok(done)
}

/// The destination of `target`, and an error unless the policy says live there: edits,
/// deletes, reactions and ephemeral messages change what others see at once.
async fn live_only(slack: &Slack, cfg: &Config, target: &Target) -> Result<String, String> {
    let dest = resolve(slack, target).await?;
    match decide(&cfg.policy, &dest).0 {
        Action::Live => Ok(dest.channel_id),
        other => Err(format!("this needs a live verdict for {}; the slack.yaml policy says {other:?}", dest.name.as_deref().unwrap_or(&dest.channel_id))),
    }
}

async fn message(slack: &Slack, cfg: &Config, link: &str) -> Result<(String, String), String> {
    let l = permalink(link)?;
    let channel = live_only(slack, cfg, &Target::ChannelId(l.channel)).await?;
    Ok((channel, l.ts))
}

fn e(err: crate::client::SlackError) -> String {
    err.to_string()
}

/// Edit (chat.update) a message by its permalink: markdown or text.
pub async fn update(slack: &Slack, cfg: &Config, link: &str, markdown: Option<&str>, text: Option<&str>) -> Result<Value, String> {
    let (channel, ts) = message(slack, cfg, link).await?;
    let mut body = serde_json::json!({ "channel": channel, "ts": ts });
    match (markdown, text) {
        (Some(md), None) => body["markdown_text"] = md.into(),
        (None, Some(t)) => body["text"] = t.into(),
        _ => return Err("give markdown or text, one of them".into()),
    }
    slack.client.call_json("chat.update", &body).await.map_err(e)
}

/// Delete (chat.delete) a message by its permalink.
pub async fn delete(slack: &Slack, cfg: &Config, link: &str) -> Result<Value, String> {
    let (channel, ts) = message(slack, cfg, link).await?;
    slack.client.call("chat.delete", &[("channel", channel), ("ts", ts)]).await.map_err(e)
}

/// React (reactions.add) to a message by its permalink.
pub async fn react(slack: &Slack, cfg: &Config, link: &str, emoji: &str) -> Result<Value, String> {
    let (channel, ts) = message(slack, cfg, link).await?;
    let name = emoji.trim().trim_matches(':').to_string();
    slack.client.call("reactions.add", &[("channel", channel), ("timestamp", ts), ("name", name)]).await.map_err(e)
}

/// A message only `user` sees in `to` (chat.postEphemeral).
pub async fn ephemeral(slack: &Slack, cfg: &Config, to: &str, user: &str, text: &str) -> Result<Value, String> {
    let channel = live_only(slack, cfg, &parse(to)?).await?;
    slack.client.call("chat.postEphemeral", &[("channel", channel), ("user", user.to_string()), ("text", text.to_string())]).await.map_err(e)
}

/// The channel id of `channel` when the policy's verdict there is one of `ok`.
async fn verdict_in(slack: &Slack, cfg: &Config, channel: &str, ok: &[Action], what: &str) -> Result<String, String> {
    let dest = resolve(slack, &parse(channel)?).await?;
    let verdict = decide(&cfg.policy, &dest).0;
    if ok.contains(&verdict) {
        Ok(dest.channel_id)
    } else {
        Err(format!("{what} in {} is refused: the slack.yaml policy says {verdict:?}", dest.name.as_deref().unwrap_or(&dest.channel_id)))
    }
}

/// Delete a composer draft (drafts.delete): the channel must allow drafts or live posts.
pub async fn draft_delete(slack: &Slack, cfg: &Config, draft_id: &str, channel: &str) -> Result<Value, String> {
    let ch = verdict_in(slack, cfg, channel, &[Action::Draft, Action::Live], "deleting a draft").await?;
    slack.client.call("drafts.delete", &[("id", draft_id.to_string()), ("channel", ch)]).await.map_err(e)
}

/// Cancel a scheduled message (chat.deleteScheduledMessage): the channel must be live.
pub async fn schedule_delete(slack: &Slack, cfg: &Config, scheduled_message_id: &str, channel: &str) -> Result<Value, String> {
    let ch = verdict_in(slack, cfg, channel, &[Action::Live], "cancelling a scheduled message").await?;
    slack.client.call("chat.deleteScheduledMessage", &[("channel", ch), ("scheduled_message_id", scheduled_message_id.to_string())]).await.map_err(e)
}
