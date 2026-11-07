//! gdrive: Google Drive as a CLI and an MCP server. Local paths only ever come from Drive
//! names through `file_stem`, so a shared folder cannot write outside the target directory.
use base64::Engine;
use gworkspace_api::{
    drive::{self, Drive, FileMeta, Quota},
    file_stem,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::{json, Value};

use crate::{cli::tool_result, context::Context};

pub const EXPORT_MIME: &str = "application/pdf";

async fn client(ctx: &Context) -> Result<Drive, String> {
    Ok(Drive { session: ctx.session("drive", drive::BASE).await? })
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// A download as JSON: `text` when it is UTF-8, else `base64_content` (pure).
pub fn content_json(meta: &FileMeta, bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(text) => json!({ "file_id": meta.id, "name": meta.name, "text": text }),
        Err(e) => json!({ "file_id": meta.id, "name": meta.name, "base64_content": base64::engine::general_purpose::STANDARD.encode(e.into_bytes()) }),
    }
}

/// A local relative path for walked Drive names: every segment through `file_stem`, the id
/// when nothing is left of a name (pure).
pub fn local_path(names: &[String], ids: &[String]) -> std::path::PathBuf {
    names.iter().zip(ids).map(|(n, id)| file_stem(n, id)).collect()
}

async fn folder_of(c: &Drive, parent_id: Option<String>, path: Option<String>) -> Result<Option<String>, String> {
    match (parent_id, path) {
        (Some(_), Some(_)) => Err("give either parent_id or path, not both".into()),
        (Some(id), None) => Ok(Some(id)),
        (None, Some(p)) => c.resolve_path(&p).await.map(Some).map_err(s),
        (None, None) => Ok(None),
    }
}

/// Usage: of a folder (walked), of the trash, or the whole quota.
pub async fn usage(c: &Drive, folder_id: Option<&str>, trash: bool) -> Result<Value, String> {
    match (folder_id, trash) {
        (Some(_), true) => Err("folder_id and trash cannot both be set".into()),
        (Some(id), false) => {
            let files: Vec<_> = c.walk(id).await.map_err(s)?.into_iter().filter(|e| !e.meta.is_folder()).collect();
            let total: u64 = files.iter().filter_map(|e| e.meta.size.as_deref()?.parse::<u64>().ok()).sum();
            Ok(json!({ "folder_id": id, "file_count": files.len(), "total_bytes": total }))
        }
        (None, true) => Ok(json!({ "usage_in_drive_trash": c.quota().await.map_err(s)?.usage_in_drive_trash })),
        (None, false) => serde_json::to_value::<Quota>(c.quota().await.map_err(s)?).map_err(s),
    }
}

fn fifty() -> usize {
    50
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Folder id; or `path` instead; neither: everything.
    pub parent_id: Option<String>,
    /// A /-separated folder path from My Drive's root.
    pub path: Option<String>,
    /// At most this many (default 50).
    #[serde(default = "fifty")]
    pub max_results: usize,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Words the name contains.
    pub query: String,
    pub mime_type: Option<String>,
    #[serde(default = "fifty")]
    pub max_results: usize,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct IdArgs {
    pub file_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DownloadArgs {
    pub file_id: String,
    /// Export type for Docs/Sheets/Slides (default application/pdf).
    pub export_mime_type: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UploadArgs {
    pub name: String,
    pub mime_type: String,
    /// The content, as text (the CLI uploads binary files and directories).
    pub content: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MoveArgs {
    pub file_id: String,
    pub new_parent_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UsageArgs {
    /// Sum one folder's files instead of the account quota.
    pub folder_id: Option<String>,
    /// Only the trash's usage.
    #[serde(default)]
    pub trash: bool,
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
    #[tool(description = "Files in a folder (by id or path), newest first.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn list(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        let r: Result<Vec<FileMeta>, String> = async {
            let c = client(&self.ctx).await?;
            let folder = folder_of(&c, a.parent_id, a.path).await?;
            c.list(folder.as_deref(), a.max_results).await.map_err(s)
        }
        .await;
        tool_result(r)
    }

    #[tool(description = "Files whose name contains the query, optionally of one MIME type.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn search(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        let r: Result<Vec<FileMeta>, String> = async { client(&self.ctx).await?.find(&a.query, a.mime_type.as_deref(), a.max_results).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "One file's metadata.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.metadata(&a.file_id).await.map_err(s) }.await)
    }

    #[tool(description = "A file's content (Docs/Sheets/Slides exported): text when UTF-8, else base64_content.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn download(&self, Parameters(a): Parameters<DownloadArgs>) -> CallToolResult {
        tool_result(
            async {
                let c = client(&self.ctx).await?;
                let meta = c.metadata(&a.file_id).await.map_err(s)?;
                let bytes = c.fetch(&meta, a.export_mime_type.as_deref().unwrap_or(EXPORT_MIME)).await.map_err(s)?;
                Ok(content_json(&meta, bytes))
            }
            .await,
        )
    }

    #[tool(description = "Upload text content as a new file.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn upload(&self, Parameters(a): Parameters<UploadArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.upload(&a.name, &a.mime_type, a.content.as_bytes(), a.parent_id.as_deref()).await.map_err(s) }.await)
    }

    #[tool(description = "Move a file into another folder (out of its current ones).", annotations(read_only_hint = false, destructive_hint = false))]
    async fn move_file(&self, Parameters(a): Parameters<MoveArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.move_to(&a.file_id, &a.new_parent_id).await.map_err(s) }.await)
    }

    #[tool(description = "Move a file to the trash (recoverable).", annotations(read_only_hint = false, destructive_hint = true))]
    async fn trash(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        tool_result(
            async {
                client(&self.ctx).await?.trash(&a.file_id).await.map_err(s)?;
                Ok(json!({ "file_id": a.file_id, "trashed": true }))
            }
            .await,
        )
    }

    #[tool(description = "Delete a file irrecoverably.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn delete(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        tool_result(
            async {
                client(&self.ctx).await?.delete(&a.file_id).await.map_err(s)?;
                Ok(json!({ "file_id": a.file_id, "deleted": true }))
            }
            .await,
        )
    }

    #[tool(description = "Storage: the account quota, the trash only, or one folder's total.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn usage(&self, Parameters(a): Parameters<UsageArgs>) -> CallToolResult {
        tool_result(async { usage(&client(&self.ctx).await?, a.folder_id.as_deref(), a.trash).await }.await)
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Google Drive: list, search, read, upload, move, trash and delete files; storage usage.")
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
        List {
            #[arg(long, conflicts_with = "path")]
            parent_id: Option<String>,
            /// A /-separated folder path from My Drive's root.
            #[arg(long)]
            path: Option<String>,
            #[arg(long, default_value_t = 50)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        Search {
            query: String,
            #[arg(long)]
            mime_type: Option<String>,
            #[arg(long, default_value_t = 50)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        Get {
            file_id: String,
            #[command(flatten)]
            format: Format,
        },
        /// A file, or a whole folder tree, to --output (default: its sanitised name).
        Download {
            file_id: String,
            #[arg(long)]
            output: Option<std::path::PathBuf>,
            #[arg(long)]
            export_mime_type: Option<String>,
        },
        /// A file, or a directory tree (as folders), into --parent-id.
        Upload {
            path: std::path::PathBuf,
            #[arg(long)]
            parent_id: Option<String>,
            #[arg(long)]
            mime_type: Option<String>,
        },
        Move {
            file_id: String,
            new_parent_id: String,
        },
        Trash {
            file_id: String,
        },
        Delete {
            file_id: String,
        },
        Usage {
            #[arg(long, conflicts_with = "trash")]
            folder_id: Option<String>,
            #[arg(long)]
            trash: bool,
        },
    }

    fn human(files: &[FileMeta]) -> String {
        let mut out = vec![format!("{} file(s):", files.len())];
        out.extend(files.iter().map(|f| format!("  {}{} ({})", f.name, if f.is_folder() { "/" } else { "" }, f.id)));
        out.join("\n")
    }

    fn mime_of(path: &std::path::Path) -> &'static str {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("txt" | "md" | "org") => "text/plain",
            Some("json") => "application/json",
            Some("csv") => "text/csv",
            Some("pdf") => "application/pdf",
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("zip") => "application/zip",
            _ => "application/octet-stream",
        }
    }

    fn upload_tree<'a>(c: &'a Drive, dir: &'a std::path::Path, parent: Option<&'a str>, count: &'a mut usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| format!("{}: no directory name", dir.display()))?;
            let folder = c.create_folder(&name, parent).await.map_err(s)?;
            let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(Result::ok).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let path = e.path();
                let kind = e.file_type().map_err(|err| format!("{}: {err}", path.display()))?;
                if kind.is_dir() {
                    upload_tree(c, &path, Some(&folder.id), count).await?;
                } else if kind.is_file() {
                    let bytes = std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
                    c.upload(&e.file_name().to_string_lossy(), mime_of(&path), &bytes, Some(&folder.id)).await.map_err(s)?;
                    *count += 1;
                }
            }
            Ok(())
        })
    }

    pub async fn run(ctx: &Context, command: Command) -> Result<String, String> {
        let c = client(ctx).await?;
        match command {
            Command::Common(_) => unreachable!("handled by cli::common"),
            Command::List { parent_id, path, max_results, format } => {
                let folder = folder_of(&c, parent_id, path).await?;
                Ok(render(&c.list(folder.as_deref(), max_results).await.map_err(s)?, format, |f| human(f)))
            }
            Command::Search { query, mime_type, max_results, format } => {
                Ok(render(&c.find(&query, mime_type.as_deref(), max_results).await.map_err(s)?, format, |f| human(f)))
            }
            Command::Get { file_id, format } => {
                let m = c.metadata(&file_id).await.map_err(s)?;
                Ok(render(&m, format, |m| serde_yaml_ng::to_string(m).expect("serialisable").trim_end().to_string()))
            }
            Command::Download { file_id, output, export_mime_type } => {
                let export = export_mime_type.as_deref().unwrap_or(EXPORT_MIME);
                let meta = c.metadata(&file_id).await.map_err(s)?;
                let dest = output.unwrap_or_else(|| file_stem(&meta.name, &meta.id).into());
                let write = |path: &std::path::Path, bytes: &[u8]| -> Result<(), String> {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
                };
                if !meta.is_folder() {
                    write(&dest, &c.fetch(&meta, export).await.map_err(s)?)?;
                    return Ok(format!("[+] wrote {}", dest.display()));
                }
                let tree = c.walk(&meta.id).await.map_err(s)?;
                let mut ids: std::collections::HashMap<Vec<String>, String> = std::collections::HashMap::new();
                let mut files = 0;
                for entry in &tree {
                    ids.insert(entry.path.clone(), entry.meta.id.clone());
                    let segment_ids: Vec<String> = (1..=entry.path.len()).map(|n| ids.get(&entry.path[..n]).cloned().unwrap_or_default()).collect();
                    let local = dest.join(local_path(&entry.path, &segment_ids));
                    if entry.meta.is_folder() {
                        std::fs::create_dir_all(&local).map_err(|e| format!("{}: {e}", local.display()))?;
                    } else {
                        write(&local, &c.fetch(&entry.meta, export).await.map_err(s)?)?;
                        files += 1;
                    }
                }
                Ok(format!("[+] wrote {files} file(s) of {} to {}", meta.name, dest.display()))
            }
            Command::Upload { path, parent_id, mime_type } => {
                if path.is_dir() {
                    let mut count = 0;
                    upload_tree(&c, &path, parent_id.as_deref(), &mut count).await?;
                    return Ok(format!("[+] uploaded {count} file(s) from {}", path.display()));
                }
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "upload".into());
                let m = c.upload(&name, mime_type.as_deref().unwrap_or(mime_of(&path)), &bytes, parent_id.as_deref()).await.map_err(s)?;
                Ok(format!("Uploaded: {} ({})", m.name, m.id))
            }
            Command::Move { file_id, new_parent_id } => {
                let m = c.move_to(&file_id, &new_parent_id).await.map_err(s)?;
                Ok(format!("Moved: {} ({})", m.name, m.id))
            }
            Command::Trash { file_id } => c.trash(&file_id).await.map(|()| format!("Trashed {file_id}")).map_err(s),
            Command::Delete { file_id } => c.delete(&file_id).await.map(|()| format!("Deleted {file_id}")).map_err(s),
            Command::Usage { folder_id, trash } => Ok(serde_json::to_string_pretty(&usage(&c, folder_id.as_deref(), trash).await?).expect("serialisable")),
        }
    }
}
