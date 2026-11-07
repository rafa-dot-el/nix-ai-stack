//! gtasks: Google Tasks as a CLI and an MCP server. `--list`/`list` take an id or a title.
use gworkspace_api::tasks::{Task, TaskList, TaskUpdate, Tasks, BASE};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};

use crate::cli::tool_result;

pub use crate::context::Context;

async fn client(ctx: &Context) -> Result<Tasks, String> {
    Ok(Tasks { session: ctx.session("tasks", BASE).await? })
}

pub fn human_lists(lists: &[TaskList]) -> String {
    let mut out = vec![format!("{} task list(s):", lists.len())];
    out.extend(lists.iter().map(|l| format!("  {} ({})", l.title.as_deref().unwrap_or(""), l.id.as_deref().unwrap_or(""))));
    out.join("\n")
}

pub fn human_tasks(tasks: &[Task]) -> String {
    let mut out = vec![format!("{} task(s):", tasks.len())];
    out.extend(tasks.iter().map(|t| {
        format!("  [{}] {} ({})", t.status.as_deref().unwrap_or("?"), t.title.as_deref().unwrap_or(""), t.id.as_deref().unwrap_or(""))
    }));
    out.join("\n")
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Task list id or title; without it, the task lists themselves are listed.
    pub list: Option<String>,
    #[serde(default)]
    pub show_completed: bool,
    /// At most this many entries (default 50).
    pub max_results: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskRef {
    /// Task list id or title.
    pub list: String,
    pub task_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateArgs {
    /// Task list id or title.
    pub list: String,
    pub title: String,
    pub notes: Option<String>,
    /// RFC 3339, e.g. 2026-12-01T00:00:00Z.
    pub due: Option<String>,
    /// Parent task id, to create a subtask.
    pub parent: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateArgs {
    /// Task list id or title.
    pub list: String,
    pub task_id: String,
    pub title: Option<String>,
    pub notes: Option<String>,
    /// RFC 3339; an empty string clears the due date.
    pub due: Option<String>,
    /// needsAction or completed.
    pub status: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TitleArgs {
    pub title: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRef {
    /// Task list id or title.
    pub list: String,
}

/// The MCP server: list, get, create, update, delete, create_list, delete_list.
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

    async fn run<T: serde::Serialize, F, Fut>(&self, f: F) -> CallToolResult
    where
        F: FnOnce(Tasks) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        tool_result(match client(&self.ctx).await {
            Ok(c) => f(c).await,
            Err(e) => Err(e),
        })
    }
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tool_router]
impl Server {
    #[tool(description = "List task lists, or the tasks of one list (by id or title).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn list(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        let max = a.max_results.unwrap_or(50);
        self.run(|c| async move {
            Ok(match a.list {
                None => serde_json::to_value(c.lists().await.map_err(s)?.into_iter().take(max).collect::<Vec<_>>()).map_err(s)?,
                Some(list) => {
                    let id = c.resolve_list(&list).await.map_err(s)?;
                    serde_json::to_value(c.tasks(&id, a.show_completed).await.map_err(s)?.into_iter().take(max).collect::<Vec<_>>()).map_err(s)?
                }
            })
        })
        .await
    }

    #[tool(description = "Get one task.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get(&self, Parameters(a): Parameters<TaskRef>) -> CallToolResult {
        self.run(|c| async move {
            let id = c.resolve_list(&a.list).await.map_err(s)?;
            c.get(&id, &a.task_id).await.map_err(s)
        })
        .await
    }

    #[tool(description = "Create a task (optionally a subtask, with notes and a due date).", annotations(read_only_hint = false, destructive_hint = false))]
    async fn create(&self, Parameters(a): Parameters<CreateArgs>) -> CallToolResult {
        self.run(|c| async move {
            let id = c.resolve_list(&a.list).await.map_err(s)?;
            c.create(&id, &a.title, a.notes.as_deref(), a.due.as_deref(), a.parent.as_deref()).await.map_err(s)
        })
        .await
    }

    #[tool(description = "Update a task's title, notes, due date or status; unspecified fields stay.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn update(&self, Parameters(a): Parameters<UpdateArgs>) -> CallToolResult {
        self.run(|c| async move {
            let id = c.resolve_list(&a.list).await.map_err(s)?;
            let update = TaskUpdate { title: a.title, notes: a.notes, due: a.due, status: a.status };
            c.update(&id, &a.task_id, &update).await.map_err(s)
        })
        .await
    }

    #[tool(description = "Delete a task.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn delete(&self, Parameters(a): Parameters<TaskRef>) -> CallToolResult {
        self.run(|c| async move {
            let id = c.resolve_list(&a.list).await.map_err(s)?;
            c.delete(&id, &a.task_id).await.map_err(s)?;
            Ok(serde_json::json!({ "deleted": a.task_id }))
        })
        .await
    }

    #[tool(description = "Create a task list.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn create_list(&self, Parameters(a): Parameters<TitleArgs>) -> CallToolResult {
        self.run(|c| async move { c.create_list(&a.title).await.map_err(s) }).await
    }

    #[tool(description = "Delete a task list and its tasks.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn delete_list(&self, Parameters(a): Parameters<ListRef>) -> CallToolResult {
        self.run(|c| async move {
            let id = c.resolve_list(&a.list).await.map_err(s)?;
            c.delete_list(&id).await.map_err(s)?;
            Ok(serde_json::json!({ "deleted": id }))
        })
        .await
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Google Tasks: lists and tasks. A list is named by id or title.")
    }
}

/// The CLI's own subcommands (beside auth, token, status, serve).
pub mod commands {
    use super::*;
    use crate::output::{render, Format};

    fn one(t: &Task) -> String {
        format!("{} ({})", t.title.as_deref().unwrap_or(""), t.id.as_deref().unwrap_or(""))
    }

    fn s<E: std::fmt::Display>(e: E) -> String {
        e.to_string()
    }

    #[derive(Debug, clap::Subcommand)]
    pub enum Command {
        #[command(flatten)]
        Common(crate::cli::Common),
        /// Task lists, or the tasks of --list.
        List {
            #[arg(long)]
            list: Option<String>,
            #[arg(long)]
            show_completed: bool,
            #[arg(long, default_value_t = 50)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        Get {
            task_id: String,
            #[arg(long)]
            list: String,
            #[command(flatten)]
            format: Format,
        },
        Create {
            title: String,
            #[arg(long)]
            list: String,
            #[arg(long)]
            notes: Option<String>,
            /// RFC 3339
            #[arg(long)]
            due: Option<String>,
            #[arg(long)]
            parent: Option<String>,
            #[command(flatten)]
            format: Format,
        },
        Update {
            task_id: String,
            #[arg(long)]
            list: String,
            #[arg(long)]
            title: Option<String>,
            #[arg(long)]
            notes: Option<String>,
            /// RFC 3339; "" clears it
            #[arg(long)]
            due: Option<String>,
            #[arg(long, value_parser = ["needsAction", "completed"])]
            status: Option<String>,
            #[command(flatten)]
            format: Format,
        },
        Delete {
            task_id: String,
            #[arg(long)]
            list: String,
        },
        CreateList {
            title: String,
            #[command(flatten)]
            format: Format,
        },
        DeleteList {
            #[arg(long)]
            list: String,
        },
    }

    pub async fn run(ctx: &Context, command: Command) -> Result<String, String> {
        let c = client(ctx).await?;
        match command {
            Command::Common(_) => unreachable!("handled by cli::common"),
            Command::List { list, show_completed, max_results, format } => match list {
                None => {
                    let lists: Vec<_> = c.lists().await.map_err(s)?.into_iter().take(max_results).collect();
                    Ok(render(&lists, format, |l| human_lists(l)))
                }
                Some(list) => {
                    let id = c.resolve_list(&list).await.map_err(s)?;
                    let tasks: Vec<_> = c.tasks(&id, show_completed).await.map_err(s)?.into_iter().take(max_results).collect();
                    Ok(render(&tasks, format, |t| human_tasks(t)))
                }
            },
            Command::Get { task_id, list, format } => {
                let id = c.resolve_list(&list).await.map_err(s)?;
                let task = c.get(&id, &task_id).await.map_err(s)?;
                Ok(render(&task, format, |t| serde_yaml_ng::to_string(t).expect("serialisable").trim_end().to_string()))
            }
            Command::Create { title, list, notes, due, parent, format } => {
                let id = c.resolve_list(&list).await.map_err(s)?;
                let task = c.create(&id, &title, notes.as_deref(), due.as_deref(), parent.as_deref()).await.map_err(s)?;
                Ok(render(&task, format, |t| format!("Created: {}", one(t))))
            }
            Command::Update { task_id, list, title, notes, due, status, format } => {
                let id = c.resolve_list(&list).await.map_err(s)?;
                let task = c.update(&id, &task_id, &TaskUpdate { title, notes, due, status }).await.map_err(s)?;
                Ok(render(&task, format, |t| format!("Updated: {}", one(t))))
            }
            Command::Delete { task_id, list } => {
                let id = c.resolve_list(&list).await.map_err(s)?;
                c.delete(&id, &task_id).await.map_err(s)?;
                Ok(format!("Deleted: {task_id}"))
            }
            Command::CreateList { title, format } => {
                let l = c.create_list(&title).await.map_err(s)?;
                Ok(render(&l, format, |l| format!("Created list: {} ({})", l.title.as_deref().unwrap_or(""), l.id.as_deref().unwrap_or(""))))
            }
            Command::DeleteList { list } => {
                let id = c.resolve_list(&list).await.map_err(s)?;
                c.delete_list(&id).await.map_err(s)?;
                Ok(format!("Deleted list: {id}"))
            }
        }
    }
}
