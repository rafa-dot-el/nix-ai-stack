//! gmail: Gmail as a CLI and an MCP server. The MCP tools never write files: messages and
//! attachments come back inline. Only the CLI writes, into the directory its user names,
//! with attachment names (chosen by the sender) through `file_stem`.
use base64::Engine;
use gworkspace_api::{
    file_stem,
    gmail::{self, draft_raw, Draft, DraftSummary, Gmail, Label, ThreadSummary, ThreadView},
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::{json, Value};

use crate::{cli::tool_result, context::Context};

pub const INBOX: &str = "in:inbox";

async fn client(ctx: &Context) -> Result<Gmail, String> {
    Ok(Gmail { session: ctx.session("gmail", gmail::BASE).await? })
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Bytes as `{text}` when UTF-8, else `{base64_content}` (pure).
pub fn inline(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(text) => json!({ "text": text }),
        Err(e) => json!({ "base64_content": base64::engine::general_purpose::STANDARD.encode(e.into_bytes()) }),
    }
}

fn twenty() -> usize {
    20
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// At most this many threads (default 20, at most 100).
    #[serde(default = "twenty")]
    pub max_results: usize,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Gmail search syntax, e.g. from:a@b.org newer_than:7d.
    pub query: String,
    #[serde(default = "twenty")]
    pub max_results: usize,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ThreadArgs {
    pub thread_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DraftArgs {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    /// Make it a reply in this thread.
    pub thread_id: Option<String>,
    /// Message-ID header of the message replied to.
    pub in_reply_to: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NameArgs {
    pub name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LabelArgs {
    /// Message or thread id.
    pub id: String,
    #[serde(default)]
    pub add_label_ids: Vec<String>,
    #[serde(default)]
    pub remove_label_ids: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MessageArgs {
    pub message_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AttachmentArgs {
    pub message_id: String,
    pub attachment_id: String,
}

#[derive(Clone)]
pub struct Server {
    pub ctx: Context,
    #[expect(dead_code, reason = "read by the tool_handler macro")]
    tool_router: ToolRouter<Self>,
}

impl Server {
    pub fn new(ctx: Context) -> Server {
        Server { ctx, tool_router: Self::tool_router() }
    }
}

#[tool_router]
impl Server {
    #[tool(description = "Threads in the inbox, newest first.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn list(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        let r: Result<Vec<ThreadSummary>, String> = async { client(&self.ctx).await?.threads(INBOX, a.max_results).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "Threads matching a Gmail search query.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn search(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        let r: Result<Vec<ThreadSummary>, String> = async { client(&self.ctx).await?.threads(&a.query, a.max_results).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "A thread's messages: headers, plain-text bodies, attachment lists.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get(&self, Parameters(a): Parameters<ThreadArgs>) -> CallToolResult {
        let r: Result<ThreadView, String> = async { client(&self.ctx).await?.thread(&a.thread_id).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "Existing drafts.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn list_drafts(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        let r: Result<Vec<DraftSummary>, String> = async { client(&self.ctx).await?.drafts(a.max_results).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "Create a plain-text draft (never sent from here); header values may not contain line breaks.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn create_draft(&self, Parameters(a): Parameters<DraftArgs>) -> CallToolResult {
        let d = Draft { to: a.to, cc: a.cc, bcc: a.bcc, subject: a.subject, body: a.body, in_reply_to: a.in_reply_to };
        let r: Result<DraftSummary, String> = async {
            draft_raw(&d)?;
            client(&self.ctx).await?.create_draft(&d, a.thread_id.as_deref()).await.map_err(s)
        }
        .await;
        tool_result(r)
    }

    #[tool(description = "Labels (ids and names).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn list_labels(&self) -> CallToolResult {
        let r: Result<Vec<Label>, String> = async { client(&self.ctx).await?.labels().await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "Create a label.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn create_label(&self, Parameters(a): Parameters<NameArgs>) -> CallToolResult {
        let r: Result<Label, String> = async { client(&self.ctx).await?.create_label(&a.name).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "Add or remove labels on one message (removing INBOX archives it).", annotations(read_only_hint = false, destructive_hint = false))]
    async fn modify_message_labels(&self, Parameters(a): Parameters<LabelArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.modify_labels("messages", &a.id, &a.add_label_ids, &a.remove_label_ids).await.map_err(s) }.await)
    }

    #[tool(description = "Add or remove labels on every message of a thread.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn modify_thread_labels(&self, Parameters(a): Parameters<LabelArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.modify_labels("threads", &a.id, &a.add_label_ids, &a.remove_label_ids).await.map_err(s) }.await)
    }

    #[tool(description = "A message as raw RFC 5322 (eml, or eml_base64 when not UTF-8) with its attachment list; nothing is written to disk.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn download_message(&self, Parameters(a): Parameters<MessageArgs>) -> CallToolResult {
        tool_result(
            async {
                let (view, raw) = client(&self.ctx).await?.message(&a.message_id).await.map_err(s)?;
                let mut out = json!({ "message_id": view.id, "attachments": view.attachments });
                match String::from_utf8(raw) {
                    Ok(eml) => out["eml"] = eml.into(),
                    Err(e) => out["eml_base64"] = base64::engine::general_purpose::STANDARD.encode(e.into_bytes()).into(),
                }
                Ok(out)
            }
            .await,
        )
    }

    #[tool(description = "One attachment's content: text when UTF-8, else base64_content.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get_attachment(&self, Parameters(a): Parameters<AttachmentArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.attachment(&a.message_id, &a.attachment_id).await.map(inline).map_err(s) }.await)
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Gmail: read threads, export messages and attachments inline, manage labels, create drafts (never send).")
    }
}

/// The CLI's own subcommands (beside auth, token, status, serve).
pub mod commands {
    use super::*;
    use crate::output::{render, Format};

    #[derive(Debug, clap::Subcommand)]
    pub enum Command {
        #[command(flatten)]
        Common(crate::cli::Common),
        /// Inbox threads.
        List {
            #[arg(long, default_value_t = 20)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        /// Threads matching QUERY (Gmail syntax); none: the inbox.
        Search {
            query: Option<String>,
            #[arg(long, default_value_t = 20)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        Get {
            thread_id: String,
            #[command(flatten)]
            format: Format,
        },
        /// A message and its attachments into OUTPUT: eml (message.eml, attachments/) or maildir.
        Download {
            message_id: String,
            #[arg(long)]
            output: std::path::PathBuf,
            #[arg(long, default_value = "eml", value_parser = ["eml", "maildir"])]
            format: String,
        },
        ListDrafts {
            #[arg(long, default_value_t = 20)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        CreateDraft {
            #[arg(long)]
            to: String,
            #[arg(long)]
            subject: String,
            #[arg(long)]
            body: String,
            #[arg(long)]
            cc: Option<String>,
            #[arg(long)]
            bcc: Option<String>,
            #[arg(long)]
            thread_id: Option<String>,
            #[arg(long)]
            in_reply_to: Option<String>,
        },
        ListLabels {
            #[command(flatten)]
            format: Format,
        },
        CreateLabel {
            name: String,
        },
        ModifyMessageLabels {
            id: String,
            #[arg(long = "add")]
            add: Vec<String>,
            #[arg(long = "remove")]
            remove: Vec<String>,
        },
        ModifyThreadLabels {
            id: String,
            #[arg(long = "add")]
            add: Vec<String>,
            #[arg(long = "remove")]
            remove: Vec<String>,
        },
    }

    fn threads(t: &[ThreadSummary]) -> String {
        let mut out = vec![format!("{} thread(s):", t.len())];
        out.extend(t.iter().map(|x| format!("  {}  {}", x.id, x.snippet.as_deref().unwrap_or(""))));
        out.join("\n")
    }

    fn write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub async fn run(ctx: &Context, command: Command) -> Result<String, String> {
        if let Command::CreateDraft { to, subject, body, cc, bcc, thread_id, in_reply_to } = command {
            let d = Draft { to, cc, bcc, subject, body, in_reply_to };
            draft_raw(&d)?;
            let r = client(ctx).await?.create_draft(&d, thread_id.as_deref()).await.map_err(s)?;
            return Ok(format!("Draft {} created", r.id));
        }
        let c = client(ctx).await?;
        match command {
            Command::Common(_) | Command::CreateDraft { .. } => unreachable!("handled above"),
            Command::List { max_results, format } => Ok(render(&c.threads(INBOX, max_results).await.map_err(s)?, format, |t| threads(t))),
            Command::Search { query, max_results, format } => {
                Ok(render(&c.threads(query.as_deref().unwrap_or(INBOX), max_results).await.map_err(s)?, format, |t| threads(t)))
            }
            Command::Get { thread_id, format } => {
                let t = c.thread(&thread_id).await.map_err(s)?;
                Ok(render(&t, format, |t| {
                    t.messages
                        .iter()
                        .map(|m| format!("From: {}\nSubject: {}\nDate: {}\n\n{}", m.from.as_deref().unwrap_or(""), m.subject.as_deref().unwrap_or(""), m.date.as_deref().unwrap_or(""), m.body_text.trim_end()))
                        .collect::<Vec<_>>()
                        .join("\n\n----\n\n")
                }))
            }
            Command::Download { message_id, output, format } => {
                let (view, raw) = c.message(&message_id).await.map_err(s)?;
                if format == "maildir" {
                    for sub in ["tmp", "new", "cur"] {
                        std::fs::create_dir_all(output.join(sub)).map_err(|e| format!("{}: {e}", output.display()))?;
                    }
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    let host = std::env::var("HOSTNAME").ok().map(|h| file_stem(&h, "localhost")).unwrap_or_else(|| "localhost".into());
                    write(&output.join("new").join(format!("{now}.{}_{}.{host}", std::process::id(), file_stem(&view.id, "message"))), &raw)?;
                } else {
                    write(&output.join("message.eml"), &raw)?;
                }
                for a in &view.attachments {
                    let bytes = c.attachment(&view.id, &a.attachment_id).await.map_err(s)?;
                    write(&output.join("attachments").join(file_stem(&a.filename, &a.attachment_id)), &bytes)?;
                }
                Ok(format!("[+] wrote message {} and {} attachment(s) to {}", view.id, view.attachments.len(), output.display()))
            }
            Command::ListDrafts { max_results, format } => {
                Ok(render(&c.drafts(max_results).await.map_err(s)?, format, |d| d.iter().map(|x| format!("  {} (message {})", x.id, x.message_id.as_deref().unwrap_or(""))).collect::<Vec<_>>().join("\n")))
            }
            Command::ListLabels { format } => Ok(render(&c.labels().await.map_err(s)?, format, |l| l.iter().map(|x| format!("  {} ({})", x.name, x.id)).collect::<Vec<_>>().join("\n"))),
            Command::CreateLabel { name } => Ok(format!("Label {} created", c.create_label(&name).await.map_err(s)?.id)),
            Command::ModifyMessageLabels { id, add, remove } => c.modify_labels("messages", &id, &add, &remove).await.map(|_| format!("Labels changed on message {id}")).map_err(s),
            Command::ModifyThreadLabels { id, add, remove } => c.modify_labels("threads", &id, &add, &remove).await.map(|_| format!("Labels changed on thread {id}")).map_err(s),
        }
    }
}
