//! The Slack MCP server: the Python's read and analysis tools under their names, and the
//! slack_* write tools, all through the slack.yaml policy.
use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::{json, Value};

use crate::{actions, analysis, read::{unix_now, Slack}, setup::{new_msg_id, Setup}, spec::Spec};

fn result(r: Result<Value, String>) -> CallToolResult {
    match r {
        Ok(v) => CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(&v).expect("json"))]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
    }
}

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

macro_rules! defaults {
    ($($name:ident: $ty:ty = $v:expr;)*) => { $(fn $name() -> $ty { $v })* };
}

defaults! {
    d_types: String = "public_channel,private_channel".into();
    d_200: usize = 200;
    d_50: usize = 50;
    d_20: usize = 20;
    d_1000: usize = 1000;
    d_1: usize = 1;
    d_10: usize = 10;
    d_30d: u64 = 30;
    d_14d: u64 = 14;
    d_7d: u64 = 7;
    d_3m: u64 = 3;
    d_last_n: String = "last_n".into();
    d_all: String = "all".into();
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    #[serde(default = "d_types")]
    pub types: String,
    #[serde(default = "d_200")]
    pub limit: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ChannelArgs {
    /// Channel id (C.../G.../D...).
    pub channel: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MembersArgs {
    pub channel: String,
    #[serde(default = "d_200")]
    pub limit: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HistoryArgs {
    pub channel: String,
    #[serde(default = "d_50")]
    pub limit: usize,
    pub oldest: Option<String>,
    pub latest: Option<String>,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RepliesArgs {
    pub channel: String,
    /// The thread's parent ts.
    pub ts: String,
    #[serde(default = "d_200")]
    pub limit: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DumpArgs {
    pub channel: String,
    /// last_n (default) or time_range.
    #[serde(default = "d_last_n")]
    pub mode: String,
    #[serde(default = "d_1000")]
    pub count: usize,
    #[serde(default = "d_3m")]
    pub months: u64,
    #[serde(default = "d_200")]
    pub chunk_size: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Slack search syntax: in:#chan from:@user before:2026-01-01 ...
    pub query: String,
    #[serde(default = "d_20")]
    pub count: usize,
    #[serde(default = "d_1")]
    pub page: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UsersArgs {
    #[serde(default = "d_1000")]
    pub limit: usize,
    #[serde(default)]
    pub include_bots: bool,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UserArgs {
    pub user_id: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryArgs {
    /// A user id, an email address, or part of a name.
    pub query: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UserChannelsArgs {
    pub user_id: String,
    #[serde(default = "d_types")]
    pub types: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct IdsArgs {
    pub user_ids: Vec<String>,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ActivityArgs {
    pub channel: String,
    #[serde(default = "d_30d")]
    pub days: u64,
    #[serde(default = "d_10")]
    pub top_n: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InteractionsArgs {
    #[serde(default = "d_30d")]
    pub days: u64,
    #[serde(default = "d_20")]
    pub limit: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TopicsArgs {
    pub channel: String,
    #[serde(default = "d_14d")]
    pub days: u64,
    #[serde(default = "d_200")]
    pub sample_size: usize,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ActionArgs {
    pub channel: String,
    #[serde(default = "d_7d")]
    pub days: u64,
    pub keywords: Option<Vec<String>>,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CacheArgs {
    /// all (default), users or channels.
    #[serde(default = "d_all")]
    pub cache_type: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SendArgs {
    /// me, @user, #channel or an id.
    pub to: String,
    pub text: Option<String>,
    pub markdown: Option<String>,
    /// Raw Block Kit.
    pub blocks: Option<Value>,
    /// Paths inside attachments.allowed_roots.
    #[serde(default)]
    pub attach: Vec<String>,
    /// Text for the shared voice, attached as audio.
    pub say: Option<String>,
    /// A message link or ts to reply in.
    pub thread: Option<String>,
    #[serde(default)]
    pub broadcast: bool,
    pub unfurl_links: Option<bool>,
    pub unfurl_media: Option<bool>,
    /// live or draft; never more than the policy allows.
    pub mode: Option<String>,
    /// Return the decision and the exact requests without sending.
    #[serde(default)]
    pub dry_run: bool,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LinkArgs {
    /// The message's link (Copy link in Slack).
    pub link: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateArgs {
    pub link: String,
    pub markdown: Option<String>,
    pub text: Option<String>,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReactArgs {
    pub link: String,
    /// e.g. +1 or :eyes:
    pub emoji: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EphemeralArgs {
    pub to: String,
    pub user: String,
    pub text: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetArgs {
    /// me, @user, #channel or an id.
    pub to: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DraftDeleteArgs {
    pub draft_id: String,
    pub channel: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScheduledDeleteArgs {
    pub scheduled_message_id: String,
    pub channel: String,
}

#[derive(Clone)]
pub struct Server {
    pub setup: Arc<Setup>,
    #[expect(dead_code, reason = "read by the tool_handler macro")]
    tool_router: ToolRouter<Self>,
}

impl Server {
    pub fn new(setup: Arc<Setup>) -> Server {
        Server { setup, tool_router: Self::tool_router() }
    }

    fn slack(&self) -> Result<&Slack, String> {
        self.setup.slack.as_ref().map_err(Clone::clone)
    }

    async fn oldest(days: u64) -> String {
        (unix_now() - days as f64 * 86_400.0).to_string()
    }
}

#[tool_router]
impl Server {
    #[tool(description = "Channels visible to the token (cached for an hour).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn conversations_list(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        result(async { self.slack()?.conversations_list(&a.types, a.limit).await.map_err(e) }.await)
    }
    #[tool(description = "One channel: name, topic, purpose, creator, size.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn conversations_info(&self, Parameters(a): Parameters<ChannelArgs>) -> CallToolResult {
        result(async { self.slack()?.conversations_info(&a.channel).await.map_err(e) }.await)
    }
    #[tool(description = "A channel's members with display names.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn conversations_members(&self, Parameters(a): Parameters<MembersArgs>) -> CallToolResult {
        result(async { self.slack()?.conversations_members(&a.channel, a.limit).await.map_err(e) }.await)
    }
    #[tool(description = "A channel's recent messages (oldest/latest are Slack ts).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn conversations_history(&self, Parameters(a): Parameters<HistoryArgs>) -> CallToolResult {
        result(async { self.slack()?.history(&a.channel, a.limit, a.oldest.as_deref(), a.latest.as_deref()).await.map_err(e) }.await)
    }
    #[tool(description = "A thread's messages.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn conversations_replies(&self, Parameters(a): Parameters<RepliesArgs>) -> CallToolResult {
        result(async { self.slack()?.replies(&a.channel, &a.ts, a.limit).await.map_err(e) }.await)
    }
    #[tool(description = "Many messages in chunks: the last count, or the last months.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn dump_recent_messages(&self, Parameters(a): Parameters<DumpArgs>) -> CallToolResult {
        result(async { self.slack()?.dump(&a.channel, &a.mode, a.count, a.months, a.chunk_size).await.map_err(e) }.await)
    }
    #[tool(description = "Search messages with Slack's search syntax.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn search_messages(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        result(async { self.slack()?.search(&a.query, a.count, a.page).await.map_err(e) }.await)
    }
    #[tool(description = "Workspace users (bots excluded unless asked).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn users_list(&self, Parameters(a): Parameters<UsersArgs>) -> CallToolResult {
        result(async { self.slack()?.users_list(a.limit, a.include_bots).await.map_err(e) }.await)
    }
    #[tool(description = "One user's profile.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn users_info(&self, Parameters(a): Parameters<UserArgs>) -> CallToolResult {
        result(async { self.slack()?.users_info(&a.user_id).await.map_err(e) }.await)
    }
    #[tool(description = "Find users by id, email or part of a name.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn find_user(&self, Parameters(a): Parameters<QueryArgs>) -> CallToolResult {
        result(async { self.slack()?.find_user(&a.query).await.map_err(e) }.await)
    }
    #[tool(description = "The channels a user is in.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn find_user_channels(&self, Parameters(a): Parameters<UserChannelsArgs>) -> CallToolResult {
        result(async { self.slack()?.find_user_channels(&a.user_id, &a.types).await.map_err(e) }.await)
    }
    #[tool(description = "Display names for user ids.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn resolve_user_ids(&self, Parameters(a): Parameters<IdsArgs>) -> CallToolResult {
        result(async { self.slack()?.resolve_user_ids(&a.user_ids).await.map_err(e) }.await)
    }
    #[tool(description = "Who posts, when (UTC hours), thread ratio, reactions, over the last days.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn channel_activity_summary(&self, Parameters(a): Parameters<ActivityArgs>) -> CallToolResult {
        result(
            async {
                let s = self.slack()?;
                let msgs = s.raw_history(&a.channel, Some(Self::oldest(a.days).await), 100_000).await.map_err(e)?;
                Ok(analysis::activity(&msgs, a.days, a.top_n, &s.names_for(&msgs).await))
            }
            .await,
        )
    }
    #[tool(description = "Recent DMs and the channels mentioning you.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn my_interactions(&self, Parameters(a): Parameters<InteractionsArgs>) -> CallToolResult {
        result(async { self.slack()?.my_interactions(a.days, a.limit).await.map_err(e) }.await)
    }
    #[tool(description = "Frequent words, shared links and busy threads of a channel.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn channel_topics_summary(&self, Parameters(a): Parameters<TopicsArgs>) -> CallToolResult {
        result(
            async {
                let msgs = self.slack()?.raw_history(&a.channel, Some(Self::oldest(a.days).await), a.sample_size).await.map_err(e)?;
                Ok(analysis::topics(&msgs))
            }
            .await,
        )
    }
    #[tool(description = "Messages that look like action items (keywords or requests).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn extract_action_items(&self, Parameters(a): Parameters<ActionArgs>) -> CallToolResult {
        result(
            async {
                let s = self.slack()?;
                let msgs = s.raw_history(&a.channel, Some(Self::oldest(a.days).await), 200).await.map_err(e)?;
                let keywords = a.keywords.unwrap_or_else(analysis::default_keywords);
                Ok(analysis::action_items(&msgs, &keywords, &a.channel, &s.names_for(&msgs).await))
            }
            .await,
        )
    }
    #[tool(description = "Whether slack is configured, the token kind, the cache.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn status(&self) -> CallToolResult {
        result(Ok(match &self.setup.slack {
            Ok(s) => json!({ "configured": true, "session_token": s.client.is_session(), "cache": s.cache_stats(), "policy_rules": self.setup.config.policy.rules.len() }),
            Err(why) => json!({ "configured": false, "reason": why }),
        }))
    }
    #[tool(description = "Drop the user and/or channel cache.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn clear_cache(&self, Parameters(a): Parameters<CacheArgs>) -> CallToolResult {
        result(self.slack().map(|s| s.clear_cache(&a.cache_type)))
    }
    #[tool(description = "Send (live, draft or scheduled per slack.yaml's policy); dry_run shows the decision and requests.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn slack_send(&self, Parameters(a): Parameters<SendArgs>) -> CallToolResult {
        result(
            async {
                let s = self.slack()?;
                let spec = Spec {
                    to: a.to,
                    text: a.text,
                    markdown: a.markdown,
                    blocks: a.blocks,
                    attach: a.attach.into_iter().map(|path| crate::spec::Attach { path, kind: None, alt: None, title: None, lang: None }).collect(),
                    say: a.say,
                    thread: a.thread,
                    broadcast: a.broadcast,
                    unfurl: crate::spec::Unfurl { links: a.unfurl_links, media: a.unfurl_media },
                    mode: a.mode,
                    ..Default::default()
                };
                actions::send_spec(s, &self.setup.config, &spec, a.dry_run, &self.setup.tts, &new_msg_id()).await
            }
            .await,
        )
    }
    #[tool(description = "Edit a message (needs a live verdict for its channel).", annotations(read_only_hint = false, destructive_hint = false))]
    async fn slack_update(&self, Parameters(a): Parameters<UpdateArgs>) -> CallToolResult {
        result(async { actions::update(self.slack()?, &self.setup.config, &a.link, a.markdown.as_deref(), a.text.as_deref()).await }.await)
    }
    #[tool(description = "Delete a message (needs a live verdict for its channel).", annotations(read_only_hint = false, destructive_hint = true))]
    async fn slack_delete(&self, Parameters(a): Parameters<LinkArgs>) -> CallToolResult {
        result(async { actions::delete(self.slack()?, &self.setup.config, &a.link).await }.await)
    }
    #[tool(description = "React to a message with an emoji.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn slack_react(&self, Parameters(a): Parameters<ReactArgs>) -> CallToolResult {
        result(async { actions::react(self.slack()?, &self.setup.config, &a.link, &a.emoji).await }.await)
    }
    #[tool(description = "A message only one user sees.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn slack_ephemeral(&self, Parameters(a): Parameters<EphemeralArgs>) -> CallToolResult {
        result(async { actions::ephemeral(self.slack()?, &self.setup.config, &a.to, &a.user, &a.text).await }.await)
    }
    #[tool(description = "Where a target goes and what the policy says (no message is sent).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn slack_resolve(&self, Parameters(a): Parameters<TargetArgs>) -> CallToolResult {
        result(
            async {
                let dest = crate::send::resolve(self.slack()?, &crate::target::parse(&a.to)?).await?;
                let (action, rule) = crate::policy::decide(&self.setup.config.policy, &dest);
                Ok(json!({ "channel": dest.channel_id, "name": dest.name, "self_dm": dest.self_dm, "action": action, "rule": rule }))
            }
            .await,
        )
    }
    #[tool(description = "Composer drafts (session tokens only).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn slack_draft_list(&self) -> CallToolResult {
        result(async { self.slack()?.client.call("drafts.list", &[]).await.map_err(e) }.await)
    }
    #[tool(description = "Delete a composer draft.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn slack_draft_delete(&self, Parameters(a): Parameters<DraftDeleteArgs>) -> CallToolResult {
        result(async { actions::draft_delete(self.slack()?, &self.setup.config, &a.draft_id, &a.channel).await }.await)
    }
    #[tool(description = "Messages scheduled by this token.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn slack_schedule_list(&self) -> CallToolResult {
        result(async { self.slack()?.client.call("chat.scheduledMessages.list", &[]).await.map_err(e) }.await)
    }
    #[tool(description = "Cancel a scheduled message.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn slack_schedule_delete(&self, Parameters(a): Parameters<ScheduledDeleteArgs>) -> CallToolResult {
        result(async { actions::schedule_delete(self.slack()?, &self.setup.config, &a.scheduled_message_id, &a.channel).await }.await)
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Slack: read, search and analyse; send, edit and react through the slack.yaml policy (live, draft or deny).")
    }
}
