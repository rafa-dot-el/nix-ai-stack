//! gdocs: Google Docs as a CLI and an MCP server. Search, export and delete go through Drive.
use gworkspace_api::{
    docs::{self, Docs, DocumentText},
    drive::{self, Drive, DriveFile, DOC_MIME},
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};

use crate::{cli::tool_result, context::Context};

const MARKDOWN: &str = "text/markdown";

async fn docs_client(ctx: &Context) -> Result<Docs, String> {
    Ok(Docs { session: ctx.session("docs", docs::BASE).await? })
}

async fn drive_client(ctx: &Context) -> Result<Drive, String> {
    Ok(Drive { session: ctx.session("drive", drive::BASE).await? })
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// A document's content: plain text from the Docs API, or Markdown from Drive's export.
pub async fn download(ctx: &Context, id: &str, markdown: bool) -> Result<String, String> {
    if markdown {
        let bytes = drive_client(ctx).await?.export(id, MARKDOWN).await.map_err(s)?;
        String::from_utf8(bytes).map_err(|e| format!("export is not UTF-8: {e}"))
    } else {
        Ok(docs_client(ctx).await?.get(id, false).await.map_err(s)?.text)
    }
}

pub async fn delete(ctx: &Context, id: &str, permanent: bool) -> Result<(), String> {
    let d = drive_client(ctx).await?;
    if permanent { d.delete(id).await } else { d.trash(id).await }.map_err(s)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DocArgs {
    pub document_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Words the title contains; none: the newest documents.
    pub query: Option<String>,
    /// At most this many (default 20, at most 100).
    pub max_results: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TitleArgs {
    pub title: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AppendArgs {
    pub document_id: String,
    pub text: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DownloadArgs {
    pub document_id: String,
    /// Markdown (Drive export) instead of plain text.
    #[serde(default)]
    pub markdown: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DeleteArgs {
    pub document_id: String,
    /// Delete irrecoverably instead of moving to the trash.
    #[serde(default)]
    pub permanent: bool,
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
    #[tool(description = "A document's plain text, with its id and title.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get_document(&self, Parameters(a): Parameters<DocArgs>) -> CallToolResult {
        tool_result(async { docs_client(&self.ctx).await?.get(&a.document_id, false).await.map_err(s) }.await)
    }

    #[tool(description = "A document's text with Markdown headings and `a | b` table rows.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get_document_structure(&self, Parameters(a): Parameters<DocArgs>) -> CallToolResult {
        tool_result(async { docs_client(&self.ctx).await?.get(&a.document_id, true).await.map_err(s) }.await)
    }

    #[tool(description = "Find Google Docs whose title contains the query, newest first.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn search_documents(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        let found: Result<Vec<DriveFile>, String> =
            async { drive_client(&self.ctx).await?.search(DOC_MIME, a.query.as_deref(), a.max_results.unwrap_or(20)).await.map_err(s) }.await;
        tool_result(found)
    }

    #[tool(description = "Create a blank Google Doc.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn new_document(&self, Parameters(a): Parameters<TitleArgs>) -> CallToolResult {
        let made: Result<DocumentText, String> = async { docs_client(&self.ctx).await?.create(&a.title).await.map_err(s) }.await;
        tool_result(made)
    }

    #[tool(description = "Append text at the end of a document; returns the inserted index range.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn append_text(&self, Parameters(a): Parameters<AppendArgs>) -> CallToolResult {
        tool_result(
            async {
                let (start, end) = docs_client(&self.ctx).await?.append(&a.document_id, &a.text).await.map_err(s)?;
                Ok(serde_json::json!({ "document_id": a.document_id, "start_index": start, "end_index": end }))
            }
            .await,
        )
    }

    #[tool(description = "A document's content as Markdown (markdown: true) or plain text.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn download_document(&self, Parameters(a): Parameters<DownloadArgs>) -> CallToolResult {
        tool_result(
            async {
                let text = download(&self.ctx, &a.document_id, a.markdown).await?;
                Ok(serde_json::json!({ "document_id": a.document_id, "format": if a.markdown { "markdown" } else { "plain_text" }, "text": text }))
            }
            .await,
        )
    }

    #[tool(description = "Move a document to the trash, or delete it irrecoverably with permanent: true.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn delete_document(&self, Parameters(a): Parameters<DeleteArgs>) -> CallToolResult {
        tool_result(
            async {
                delete(&self.ctx, &a.document_id, a.permanent).await?;
                Ok(serde_json::json!({ "document_id": a.document_id, "permanent": a.permanent }))
            }
            .await,
        )
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Google Docs: read, search, create, append to, export and delete documents.")
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
        /// A document's plain text.
        GetDocument {
            document_id: String,
            #[command(flatten)]
            format: Format,
        },
        /// A document's text with Markdown headings and table rows.
        GetDocumentStructure {
            document_id: String,
            #[command(flatten)]
            format: Format,
        },
        /// Google Docs whose title contains QUERY, newest first.
        SearchDocuments {
            query: Option<String>,
            #[arg(long, default_value_t = 20)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        /// Create a blank document.
        New {
            title: String,
            #[command(flatten)]
            format: Format,
        },
        /// Append TEXT at the end of a document.
        AppendText {
            document_id: String,
            text: String,
        },
        /// Plain text, or Markdown with --markdown; to --output FILE or stdout.
        Download {
            document_id: String,
            #[arg(long)]
            output: Option<std::path::PathBuf>,
            #[arg(long)]
            markdown: bool,
        },
        /// Move to the trash, or --permanent.
        Delete {
            document_id: String,
            #[arg(long)]
            permanent: bool,
        },
    }

    pub async fn run(ctx: &Context, command: Command) -> Result<String, String> {
        match command {
            Command::Common(_) => unreachable!("handled by cli::common"),
            Command::GetDocument { document_id, format } => {
                let doc = docs_client(ctx).await?.get(&document_id, false).await.map_err(s)?;
                Ok(render(&doc, format, |d| d.text.clone()))
            }
            Command::GetDocumentStructure { document_id, format } => {
                let doc = docs_client(ctx).await?.get(&document_id, true).await.map_err(s)?;
                Ok(render(&doc, format, |d| d.text.clone()))
            }
            Command::SearchDocuments { query, max_results, format } => {
                let found = drive_client(ctx).await?.search(DOC_MIME, query.as_deref(), max_results).await.map_err(s)?;
                Ok(render(&found, format, |f| {
                    let mut out = vec![format!("{} document(s):", f.len())];
                    out.extend(f.iter().map(|d| format!("  {} ({})", d.name, d.id)));
                    out.join("\n")
                }))
            }
            Command::New { title, format } => {
                let doc = docs_client(ctx).await?.create(&title).await.map_err(s)?;
                Ok(render(&doc, format, |d| format!("Created: {} ({})", d.title.as_deref().unwrap_or(""), d.document_id.as_deref().unwrap_or(""))))
            }
            Command::AppendText { document_id, text } => {
                let (start, end) = docs_client(ctx).await?.append(&document_id, &text).await.map_err(s)?;
                Ok(format!("Appended at {start}..{end}"))
            }
            Command::Download { document_id, output, markdown } => {
                let text = download(ctx, &document_id, markdown).await?;
                match output {
                    Some(path) => {
                        std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
                        Ok(format!("[+] wrote {}", path.display()))
                    }
                    None => Ok(text),
                }
            }
            Command::Delete { document_id, permanent } => {
                delete(ctx, &document_id, permanent).await?;
                Ok(format!("{} {document_id}", if permanent { "Deleted" } else { "Trashed" }))
            }
        }
    }
}
