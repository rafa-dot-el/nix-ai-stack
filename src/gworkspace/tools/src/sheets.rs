//! gsheets: Google Sheets as a CLI and an MCP server. Search and delete go through Drive.
use gworkspace_api::{
    drive::{self, Drive, DriveFile},
    sheets::{self, file_stem, Sheets, SpreadsheetMetadata, ValueRange, SHEET_MIME},
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::{json, Value};

use crate::{cli::tool_result, context::Context};

async fn client(ctx: &Context) -> Result<Sheets, String> {
    Ok(Sheets { session: ctx.session("sheets", sheets::BASE).await? })
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// USER_ENTERED (parsed as if typed: formulas, dates) or RAW (stored as given).
pub fn input_option(value: Option<&str>) -> Result<&str, String> {
    match value.unwrap_or("USER_ENTERED") {
        v @ ("USER_ENTERED" | "RAW") => Ok(v),
        other => Err(format!("value_input_option must be USER_ENTERED or RAW, not {other:?}")),
    }
}

/// The download payload: `single_csv` for one tab, else `tabs: [{title, csv}]`.
pub fn download_json(tabs: Vec<(String, String)>) -> Value {
    if tabs.len() == 1 {
        json!({ "single_csv": tabs.into_iter().next().expect("one").1 })
    } else {
        json!({ "tabs": tabs.into_iter().map(|(title, csv)| json!({ "title": title, "csv": csv })).collect::<Vec<_>>() })
    }
}

fn created(v: &Value) -> Value {
    json!({ "spreadsheet_id": v.get("spreadsheetId"), "title": v.pointer("/properties/title"), "url": v.get("spreadsheetUrl") })
}

fn added(v: &Value) -> Value {
    let p = v.pointer("/replies/0/addSheet/properties").cloned().unwrap_or(Value::Null);
    json!({ "sheet_id": p.get("sheetId"), "title": p.get("title"), "index": p.get("index") })
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct IdArgs {
    pub spreadsheet_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RangeArgs {
    pub spreadsheet_id: String,
    /// A1 notation, e.g. Sheet1!A1:C10 or 'My tab'!A:A.
    pub range: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RangesArgs {
    pub spreadsheet_id: String,
    pub ranges: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteArgs {
    pub spreadsheet_id: String,
    pub range: String,
    /// Rows of cells.
    pub values: Vec<Vec<Value>>,
    /// USER_ENTERED (default: formulas and dates are parsed) or RAW.
    pub value_input_option: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AppendArgs {
    pub spreadsheet_id: String,
    /// The table to append after, e.g. Sheet1.
    pub range: String,
    pub values: Vec<Vec<Value>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NewArgs {
    pub title: String,
    /// Tab titles; none: one default tab.
    #[serde(default)]
    pub sheet_titles: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AddTabArgs {
    pub spreadsheet_id: String,
    pub title: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DeleteTabArgs {
    pub spreadsheet_id: String,
    /// The tab's numeric sheet_id (see get_spreadsheet).
    pub sheet_id: i64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Words the title contains; none: the newest spreadsheets.
    pub query: Option<String>,
    /// At most this many (default 20, at most 100).
    pub max_results: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DeleteArgs {
    pub spreadsheet_id: String,
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
    #[tool(description = "Title, locale, URL and tabs (sheet_id, title, size) of a spreadsheet.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get_spreadsheet(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        let r: Result<SpreadsheetMetadata, String> = async { client(&self.ctx).await?.metadata(&a.spreadsheet_id).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "The values of one A1 range.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn read_range(&self, Parameters(a): Parameters<RangeArgs>) -> CallToolResult {
        let r: Result<ValueRange, String> = async { client(&self.ctx).await?.read(&a.spreadsheet_id, &a.range).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "The values of several A1 ranges at once.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn batch_read(&self, Parameters(a): Parameters<RangesArgs>) -> CallToolResult {
        let r: Result<Vec<ValueRange>, String> = async { client(&self.ctx).await?.batch_read(&a.spreadsheet_id, &a.ranges).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "Overwrite a range with rows of values.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn write_range(&self, Parameters(a): Parameters<WriteArgs>) -> CallToolResult {
        tool_result(
            async {
                let input = input_option(a.value_input_option.as_deref())?;
                client(&self.ctx).await?.write(&a.spreadsheet_id, &a.range, &a.values, input).await.map_err(s)
            }
            .await,
        )
    }

    #[tool(description = "Append rows after the table in a range.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn append_rows(&self, Parameters(a): Parameters<AppendArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.append(&a.spreadsheet_id, &a.range, &a.values).await.map_err(s) }.await)
    }

    #[tool(name = "new", description = "Create a spreadsheet, optionally with named tabs.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn new_spreadsheet(&self, Parameters(a): Parameters<NewArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.create(&a.title, &a.sheet_titles).await.map(|v| created(&v)).map_err(s) }.await)
    }

    #[tool(description = "Add a tab to a spreadsheet.", annotations(read_only_hint = false, destructive_hint = false))]
    async fn add_sheet(&self, Parameters(a): Parameters<AddTabArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.add_tab(&a.spreadsheet_id, &a.title).await.map(|v| added(&v)).map_err(s) }.await)
    }

    #[tool(description = "Delete a tab (and its data) by sheet_id.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn delete_sheet(&self, Parameters(a): Parameters<DeleteTabArgs>) -> CallToolResult {
        tool_result(
            async {
                client(&self.ctx).await?.delete_tab(&a.spreadsheet_id, a.sheet_id).await.map_err(s)?;
                Ok(json!({ "spreadsheet_id": a.spreadsheet_id, "deleted_sheet_id": a.sheet_id }))
            }
            .await,
        )
    }

    #[tool(description = "Every tab as CSV: single_csv for one tab, tabs: [{title, csv}] for several.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn download_spreadsheet(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.csv(&a.spreadsheet_id).await.map(download_json).map_err(s) }.await)
    }

    #[tool(description = "Find spreadsheets whose title contains the query, newest first.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn search_spreadsheets(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        let r: Result<Vec<DriveFile>, String> = async {
            let d = Drive { session: self.ctx.session("drive", drive::BASE).await? };
            d.search(SHEET_MIME, a.query.as_deref(), a.max_results.unwrap_or(20)).await.map_err(s)
        }
        .await;
        tool_result(r)
    }

    #[tool(description = "Move a spreadsheet to the trash, or delete it irrecoverably with permanent: true.", annotations(read_only_hint = false, destructive_hint = true))]
    async fn delete_spreadsheet(&self, Parameters(a): Parameters<DeleteArgs>) -> CallToolResult {
        tool_result(
            async {
                crate::docs::delete(&self.ctx, &a.spreadsheet_id, a.permanent).await?;
                Ok(json!({ "spreadsheet_id": a.spreadsheet_id, "permanent": a.permanent }))
            }
            .await,
        )
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Google Sheets: read and write A1 ranges, manage tabs, export CSV, search, delete.")
    }
}

/// The CLI's own subcommands (beside auth, token, status, serve).
pub mod commands {
    use super::*;
    use crate::output::{render, Format};

    fn values(json: &str) -> Result<Vec<Vec<Value>>, String> {
        serde_json::from_str(json).map_err(|e| format!("values: expected a 2-D JSON array like '[[\"a\",1]]': {e}"))
    }

    fn json(v: &Value) -> String {
        serde_json::to_string_pretty(v).expect("serialisable")
    }

    #[derive(Debug, clap::Subcommand)]
    pub enum Command {
        #[command(flatten)]
        Common(crate::cli::Common),
        GetSpreadsheet {
            spreadsheet_id: String,
            #[command(flatten)]
            format: Format,
        },
        ReadRange {
            spreadsheet_id: String,
            range: String,
            #[command(flatten)]
            format: Format,
        },
        BatchRead {
            spreadsheet_id: String,
            ranges: Vec<String>,
            #[command(flatten)]
            format: Format,
        },
        /// VALUES_JSON: rows, e.g. '[["a",1],["b",2]]'.
        WriteRange {
            spreadsheet_id: String,
            range: String,
            values_json: String,
            #[arg(long, default_value = "USER_ENTERED", value_parser = ["USER_ENTERED", "RAW"])]
            value_input_option: String,
        },
        AppendRows {
            spreadsheet_id: String,
            range: String,
            values_json: String,
        },
        New {
            title: String,
            #[arg(long, value_delimiter = ',')]
            sheet_titles: Vec<String>,
        },
        AddSheet {
            spreadsheet_id: String,
            title: String,
        },
        DeleteSheet {
            spreadsheet_id: String,
            sheet_id: i64,
        },
        /// One tab: OUTPUT (default <title>.csv); several: a directory OUTPUT (default <title>/).
        Download {
            spreadsheet_id: String,
            #[arg(long)]
            output: Option<std::path::PathBuf>,
        },
        SearchSpreadsheets {
            query: Option<String>,
            #[arg(long, default_value_t = 20)]
            max_results: usize,
            #[command(flatten)]
            format: Format,
        },
        Delete {
            spreadsheet_id: String,
            #[arg(long)]
            permanent: bool,
        },
    }

    fn human(meta: &SpreadsheetMetadata) -> String {
        let mut out = vec![format!("{} ({})", meta.title, meta.spreadsheet_id)];
        out.extend(meta.sheets.iter().map(|t| format!("  [{}] {} ({}x{}) sheet_id={}", t.index, t.title, t.row_count.unwrap_or(0), t.column_count.unwrap_or(0), t.sheet_id)));
        out.join("\n")
    }

    fn rows(r: &ValueRange) -> String {
        sheets::to_csv(&r.values).trim_end().replace("\r\n", "\n")
    }

    pub async fn run(ctx: &Context, command: Command) -> Result<String, String> {
        if let Command::Delete { spreadsheet_id, permanent } = &command {
            crate::docs::delete(ctx, spreadsheet_id, *permanent).await?;
            return Ok(format!("{} {spreadsheet_id}", if *permanent { "Deleted" } else { "Trashed" }));
        }
        if let Command::SearchSpreadsheets { query, max_results, format } = &command {
            let d = Drive { session: ctx.session("drive", drive::BASE).await? };
            let found = d.search(SHEET_MIME, query.as_deref(), *max_results).await.map_err(s)?;
            return Ok(render(&found, *format, |f| {
                let mut out = vec![format!("{} spreadsheet(s):", f.len())];
                out.extend(f.iter().map(|x| format!("  {} ({})", x.name, x.id)));
                out.join("\n")
            }));
        }
        let c = client(ctx).await?;
        match command {
            Command::Common(_) | Command::Delete { .. } | Command::SearchSpreadsheets { .. } => unreachable!("handled above"),
            Command::GetSpreadsheet { spreadsheet_id, format } => Ok(render(&c.metadata(&spreadsheet_id).await.map_err(s)?, format, human)),
            Command::ReadRange { spreadsheet_id, range, format } => Ok(render(&c.read(&spreadsheet_id, &range).await.map_err(s)?, format, rows)),
            Command::BatchRead { spreadsheet_id, ranges, format } => {
                let r = c.batch_read(&spreadsheet_id, &ranges).await.map_err(s)?;
                Ok(render(&r, format, |all| all.iter().map(|r| format!("{}:\n{}", r.range.as_deref().unwrap_or(""), rows(r))).collect::<Vec<_>>().join("\n\n")))
            }
            Command::WriteRange { spreadsheet_id, range, values_json, value_input_option } => {
                Ok(json(&c.write(&spreadsheet_id, &range, &values(&values_json)?, &value_input_option).await.map_err(s)?))
            }
            Command::AppendRows { spreadsheet_id, range, values_json } => Ok(json(&c.append(&spreadsheet_id, &range, &values(&values_json)?).await.map_err(s)?)),
            Command::New { title, sheet_titles } => Ok(json(&created(&c.create(&title, &sheet_titles).await.map_err(s)?))),
            Command::AddSheet { spreadsheet_id, title } => Ok(json(&added(&c.add_tab(&spreadsheet_id, &title).await.map_err(s)?))),
            Command::DeleteSheet { spreadsheet_id, sheet_id } => {
                c.delete_tab(&spreadsheet_id, sheet_id).await.map_err(s)?;
                Ok(format!("Deleted sheet {sheet_id}"))
            }
            Command::Download { spreadsheet_id, output } => {
                let meta = c.metadata(&spreadsheet_id).await.map_err(s)?;
                let stem = file_stem(&meta.title, &spreadsheet_id);
                let tabs = c.csv_of(&spreadsheet_id, &meta).await.map_err(s)?;
                let write = |path: &std::path::Path, csv: &str| std::fs::write(path, csv).map_err(|e| format!("{}: {e}", path.display()));
                if tabs.len() == 1 {
                    let path = output.unwrap_or_else(|| format!("{stem}.csv").into());
                    write(&path, &tabs[0].1)?;
                    return Ok(format!("[+] wrote {}", path.display()));
                }
                let dir = output.unwrap_or_else(|| stem.into());
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                for (i, (title, csv)) in tabs.iter().enumerate() {
                    write(&dir.join(format!("{}.csv", file_stem(title, &format!("tab-{i}")))), csv)?;
                }
                Ok(format!("[+] wrote {} CSV file(s) to {}", tabs.len(), dir.display()))
            }
        }
    }
}
