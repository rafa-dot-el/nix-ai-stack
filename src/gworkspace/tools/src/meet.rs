//! gmeet: Google Meet (read-only) as a CLI and an MCP server: meetings from Calendar,
//! conference records, participants, transcripts, recordings, attachments, transcript search.
use chrono::{Duration, Utc};
use gworkspace_api::{
    calendar, docs::{self, Docs},
    drive::{self, file_meta, quote, Drive, DriveFile, DOC_MIME},
    meet::{self, meeting, summary, Entry, Meet, Meeting, MeetingSummary, Participant, Record},
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::{json, Value};

use crate::{cli::tool_result, context::Context};

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

async fn meet_client(ctx: &Context) -> Result<Meet, String> {
    Ok(Meet { session: ctx.session("meet", meet::BASE).await? })
}

async fn drive_client(ctx: &Context) -> Result<Drive, String> {
    Ok(Drive { session: ctx.session("drive", drive::BASE).await? })
}

fn rfc3339(t: chrono::DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub async fn meetings(ctx: &Context, calendar_id: &str, past: i64, future: i64, max: usize, query: Option<&str>) -> Result<Vec<MeetingSummary>, String> {
    if past < 0 || future < 0 {
        return Err("past_days and future_days must not be negative".into());
    }
    let session = ctx.session("calendar", calendar::BASE).await?;
    let mut url = session.url(&["calendars", calendar_id, "events"]);
    {
        let now = Utc::now();
        let mut q = url.query_pairs_mut();
        q.append_pair("timeMin", &rfc3339(now - Duration::days(past)))
            .append_pair("timeMax", &rfc3339(now + Duration::days(future)))
            .append_pair("singleEvents", "true")
            .append_pair("orderBy", "startTime")
            .append_pair("maxResults", &max.clamp(1, 250).to_string());
        if let Some(query) = query {
            q.append_pair("q", query);
        }
    }
    let r: Value = session.get(&url).await.map_err(s)?;
    Ok(r.get("items").and_then(Value::as_array).into_iter().flatten().map(summary).collect())
}

pub async fn get_meeting(ctx: &Context, calendar_id: &str, event_id: &str) -> Result<Meeting, String> {
    let session = ctx.session("calendar", calendar::BASE).await?;
    let event: Value = session.get(&session.url(&["calendars", calendar_id, "events", event_id])).await.map_err(s)?;
    Ok(meeting(&event))
}

/// "entries" (speaker/text) or "document" (the transcript's Google Doc; markdown via export).
pub async fn transcript(ctx: &Context, record: &str, style: &str, markdown: bool) -> Result<Value, String> {
    let m = meet_client(ctx).await?;
    match style {
        "entries" => Ok(serde_json::to_value::<Vec<Entry>>(m.entries(record).await.map_err(s)?).map_err(s)?),
        "document" => {
            let first = m.transcripts(record).await.map_err(s)?.into_iter().next().ok_or("no transcript for this conference")?;
            let doc = first.pointer("/docsDestination/document").and_then(Value::as_str).ok_or("the transcript has no Google Doc")?.to_string();
            let text = if markdown {
                String::from_utf8_lossy(&drive_client(ctx).await?.export(&doc, "text/markdown").await.map_err(s)?).into_owned()
            } else {
                Docs { session: ctx.session("docs", docs::BASE).await? }.get(&doc, false).await.map_err(s)?.text
            };
            Ok(json!({ "document_id": doc, "text": text }))
        }
        other => Err(format!("style must be entries or document, not {other:?}")),
    }
}

/// Text content for text-like and native files (status checked); a link for binaries.
pub async fn attachment(ctx: &Context, file_id: &str) -> Result<Value, String> {
    let d = drive_client(ctx).await?;
    let mut url = d.session.url(&[file_id]);
    url.query_pairs_mut().append_pair("fields", "id,name,mimeType,size,webViewLink");
    let raw: Value = d.session.get(&url).await.map_err(s)?;
    let meta = file_meta(&raw).ok_or("the file has no id or name")?;
    let mime = meta.mime_type.clone().unwrap_or_default();
    let texty = mime.starts_with("text/") || matches!(mime.as_str(), "application/json" | "application/xml" | "application/javascript");
    if texty || meta.is_native() {
        let bytes = d.fetch(&meta, "text/plain").await.map_err(s)?;
        return Ok(json!({ "file_id": meta.id, "name": meta.name, "mime_type": mime, "content": String::from_utf8_lossy(&bytes) }));
    }
    Ok(json!({ "file_id": meta.id, "name": meta.name, "mime_type": mime, "content": null, "size": meta.size, "web_view_link": meta.web_view_link,
        "note": "binary file: use web_view_link" }))
}

pub async fn search_transcripts(ctx: &Context, query: &str, past_days: i64, max: usize) -> Result<Vec<DriveFile>, String> {
    let d = drive_client(ctx).await?;
    let cutoff = rfc3339(Utc::now() - Duration::days(past_days.max(0)));
    let q = format!("mimeType = {} and name contains {} and createdTime > {} and trashed = false", quote(DOC_MIME), quote(query), quote(&cutoff));
    let mut url = d.session.url(&[]);
    url.query_pairs_mut()
        .append_pair("q", &q)
        .append_pair("fields", "files(id, name, modifiedTime)")
        .append_pair("pageSize", &max.clamp(1, 100).to_string())
        .append_pair("orderBy", "createdTime desc");
    let r: Value = d.session.get(&url).await.map_err(s)?;
    Ok(r.get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| Some(DriveFile { id: f.get("id")?.as_str()?.into(), name: f.get("name")?.as_str()?.into(), modified_time: f.get("modifiedTime").and_then(Value::as_str).map(str::to_string) }))
        .collect())
}

fn primary() -> String {
    "primary".into()
}
fn seven() -> i64 {
    7
}
fn thirty() -> i64 {
    30
}
fn fifty() -> usize {
    50
}
fn twenty() -> usize {
    20
}
fn entries_style() -> String {
    "entries".into()
}
fn transcript_query() -> String {
    "Meeting transcript".into()
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    #[serde(default = "seven")]
    pub past_days: i64,
    #[serde(default = "seven")]
    pub future_days: i64,
    #[serde(default = "fifty")]
    pub max_results: usize,
    #[serde(default = "primary")]
    pub calendar_id: String,
    /// Free text matched against the events.
    pub query: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetArgs {
    pub event_id: String,
    #[serde(default = "primary")]
    pub calendar_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FileArgs {
    pub file_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CodeArgs {
    /// e.g. abc-mnop-xyz
    pub meeting_code: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RecordArgs {
    /// conferenceRecords/<id> (see occurrences).
    pub conference_record: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TranscriptArgs {
    pub conference_record: String,
    /// "entries" (default: speaker and text) or "document" (the transcript's Google Doc).
    #[serde(default = "entries_style")]
    pub style: String,
    /// With style document: Markdown instead of plain text.
    #[serde(default)]
    pub markdown: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    #[serde(default = "transcript_query")]
    pub query: String,
    #[serde(default = "thirty")]
    pub past_days: i64,
    #[serde(default = "twenty")]
    pub max_results: usize,
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
    #[tool(description = "Calendar events around today, with their Meet links and codes.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn list(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        tool_result(meetings(&self.ctx, &a.calendar_id, a.past_days, a.future_days, a.max_results, a.query.as_deref()).await)
    }

    #[tool(description = "One meeting: time, description, organizer, attendees, attachments.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get(&self, Parameters(a): Parameters<GetArgs>) -> CallToolResult {
        tool_result(get_meeting(&self.ctx, &a.calendar_id, &a.event_id).await)
    }

    #[tool(description = "A meeting attachment's text (Docs exported as text); binaries return a link.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn attachment(&self, Parameters(a): Parameters<FileArgs>) -> CallToolResult {
        tool_result(attachment(&self.ctx, &a.file_id).await)
    }

    #[tool(description = "Past conferences of a meeting code, with their transcripts and recordings.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn occurrences(&self, Parameters(a): Parameters<CodeArgs>) -> CallToolResult {
        let r: Result<Vec<Record>, String> = async { meet_client(&self.ctx).await?.records(&a.meeting_code).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "Who joined a conference, and when.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn participants(&self, Parameters(a): Parameters<RecordArgs>) -> CallToolResult {
        let r: Result<Vec<Participant>, String> = async { meet_client(&self.ctx).await?.participants(&a.conference_record).await.map_err(s) }.await;
        tool_result(r)
    }

    #[tool(description = "A conference's transcript: speaker entries, or its Google Doc.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn transcript(&self, Parameters(a): Parameters<TranscriptArgs>) -> CallToolResult {
        tool_result(transcript(&self.ctx, &a.conference_record, &a.style, a.markdown).await)
    }

    #[tool(description = "A conference's recordings (Drive files).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn recordings(&self, Parameters(a): Parameters<RecordArgs>) -> CallToolResult {
        tool_result(async { meet_client(&self.ctx).await?.recordings(&a.conference_record).await.map_err(s) }.await)
    }

    #[tool(description = "Transcript documents in Drive whose name contains the query, created in the last days.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn search_transcripts(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        tool_result(search_transcripts(&self.ctx, &a.query, a.past_days, a.max_results).await)
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Google Meet, read-only: meetings, conferences, participants, transcripts, recordings.")
    }
}

/// The CLI's own subcommands (beside auth, token, status, serve).
pub mod commands {
    use super::*;
    use crate::output::{render, Format};

    fn json_out(v: &Value) -> String {
        serde_json::to_string_pretty(v).expect("serialisable")
    }

    #[derive(Debug, clap::Subcommand)]
    pub enum Command {
        #[command(flatten)]
        Common(crate::cli::Common),
        List {
            #[arg(long, default_value_t = 7)]
            past_days: i64,
            #[arg(long, default_value_t = 7)]
            future_days: i64,
            #[arg(long, default_value_t = 50)]
            max_results: usize,
            #[arg(long, default_value = "primary")]
            calendar_id: String,
            #[arg(long)]
            query: Option<String>,
            #[command(flatten)]
            format: Format,
        },
        Get {
            event_id: String,
            #[arg(long, default_value = "primary")]
            calendar_id: String,
            #[command(flatten)]
            format: Format,
        },
        Attachment {
            file_id: String,
        },
        Occurrences {
            meeting_code: String,
            #[command(flatten)]
            format: Format,
        },
        Participants {
            conference_record: String,
            #[command(flatten)]
            format: Format,
        },
        Transcript {
            conference_record: String,
            #[arg(long, default_value = "entries", value_parser = ["entries", "document"])]
            style: String,
            #[arg(long)]
            markdown: bool,
        },
        Recordings {
            conference_record: String,
        },
        SearchTranscripts {
            #[arg(default_value = "Meeting transcript")]
            query: String,
            #[arg(long, default_value_t = 30)]
            past_days: i64,
            #[arg(long, default_value_t = 20)]
            max_results: usize,
        },
    }

    pub async fn run(ctx: &Context, command: Command) -> Result<String, String> {
        match command {
            Command::Common(_) => unreachable!("handled by cli::common"),
            Command::List { past_days, future_days, max_results, calendar_id, query, format } => {
                let m = meetings(ctx, &calendar_id, past_days, future_days, max_results, query.as_deref()).await?;
                Ok(render(&m, format, |m| {
                    m.iter().map(|x| format!("  {}  {} {}", x.start.as_deref().unwrap_or(""), x.title, x.meet_code.as_deref().map(|c| format!("[{c}]")).unwrap_or_default())).collect::<Vec<_>>().join("\n")
                }))
            }
            Command::Get { event_id, calendar_id, format } => {
                let m = get_meeting(ctx, &calendar_id, &event_id).await?;
                Ok(render(&m, format, |m| serde_yaml_ng::to_string(m).expect("serialisable").trim_end().to_string()))
            }
            Command::Attachment { file_id } => Ok(json_out(&attachment(ctx, &file_id).await?)),
            Command::Occurrences { meeting_code, format } => {
                let r = meet_client(ctx).await?.records(&meeting_code).await.map_err(s)?;
                Ok(render(&r, format, |r| r.iter().map(|x| format!("  {} {}", x.name.as_deref().unwrap_or(""), x.start_time.as_deref().unwrap_or(""))).collect::<Vec<_>>().join("\n")))
            }
            Command::Participants { conference_record, format } => {
                let p = meet_client(ctx).await?.participants(&conference_record).await.map_err(s)?;
                Ok(render(&p, format, |p| p.iter().map(|x| format!("  {} ({})", x.display_name.as_deref().unwrap_or("?"), x.kind)).collect::<Vec<_>>().join("\n")))
            }
            Command::Transcript { conference_record, style, markdown } => {
                let v = transcript(ctx, &conference_record, &style, markdown).await?;
                Ok(match v.get("text").and_then(Value::as_str) {
                    Some(text) => text.trim_end().to_string(),
                    None => v.as_array().into_iter().flatten().map(|e| format!("{}: {}", e["speaker"].as_str().unwrap_or(""), e["text"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n"),
                })
            }
            Command::Recordings { conference_record } => Ok(json_out(&Value::Array(meet_client(ctx).await?.recordings(&conference_record).await.map_err(s)?))),
            Command::SearchTranscripts { query, past_days, max_results } => {
                Ok(json_out(&serde_json::to_value(search_transcripts(ctx, &query, past_days, max_results).await?).map_err(s)?))
            }
        }
    }
}
