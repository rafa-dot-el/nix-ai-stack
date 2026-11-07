//! gcal: Google Calendar (read-only) as a CLI and an MCP server, with Org and iCal output.
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use gworkspace_api::calendar::{self, render_ical, render_org, timezone, window, Calendar, Event};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::json;

use crate::{cli::tool_result, context::Context};

async fn client(ctx: &Context) -> Result<Calendar, String> {
    Ok(Calendar { session: ctx.session("calendar", calendar::BASE).await? })
}

/// The display timezone: the flag, else $TZ, else /etc/localtime's zone, else UTC.
pub fn local_timezone(flag: Option<&str>, tz_env: Option<&str>) -> Result<Tz, String> {
    if let Some(name) = flag.or(tz_env.map(str::trim).filter(|t| !t.is_empty())) {
        return timezone(name);
    }
    let zone = std::fs::read_link("/etc/localtime").ok().and_then(|t| {
        let t = t.to_string_lossy().into_owned();
        t.split_once("zoneinfo/").map(|(_, z)| z.to_string())
    });
    Ok(zone.and_then(|z| timezone(&z).ok()).unwrap_or(chrono_tz::UTC))
}

fn tz_now(flag: Option<&str>) -> Result<(Tz, NaiveDate), String> {
    let tz = local_timezone(flag, std::env::var("TZ").ok().as_deref())?;
    Ok((tz, Utc::now().with_timezone(&tz).date_naive()))
}

/// [since, until] inclusive, as UTC instants in `tz`.
pub fn range(since: NaiveDate, until: NaiveDate, tz: Tz) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    let days = (until - since).num_days();
    if days < 0 {
        return Err("--until is before --since".into());
    }
    window(since, tz, 0, days)
}

async fn events_around(ctx: &Context, calendar: &str, past: i64, future: i64, tz: Option<&str>) -> Result<Vec<Event>, String> {
    let (tz, today) = tz_now(tz)?;
    let (from, until) = window(today, tz, past, future)?;
    client(ctx).await?.events(calendar, from, until).await.map_err(|e| e.to_string())
}

fn primary() -> String {
    "primary".into()
}

fn ninety() -> i64 {
    90
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Calendar id (default: primary).
    #[serde(default = "primary")]
    pub calendar_id: String,
    /// Days into the past (default 90).
    #[serde(default = "ninety")]
    pub past_days: i64,
    /// Days into the future (default 90).
    #[serde(default = "ninety")]
    pub future_days: i64,
    /// At most this many events (default 50).
    pub max_results: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetArgs {
    /// Calendar id (default: primary).
    #[serde(default = "primary")]
    pub calendar_id: String,
    pub event_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct IcalArgs {
    /// Calendar id (default: primary).
    #[serde(default = "primary")]
    pub calendar_id: String,
    #[serde(default = "ninety")]
    pub past_days: i64,
    #[serde(default = "ninety")]
    pub future_days: i64,
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
    #[tool(description = "Events in a window around today (all-day first, then by start).", annotations(read_only_hint = true, destructive_hint = false))]
    async fn list(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        let r: Result<Vec<Event>, String> = async {
            let mut evs = events_around(&self.ctx, &a.calendar_id, a.past_days, a.future_days, None).await?;
            evs.truncate(a.max_results.unwrap_or(50));
            Ok(evs)
        }
        .await;
        tool_result(r)
    }

    #[tool(description = "One event.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn get(&self, Parameters(a): Parameters<GetArgs>) -> CallToolResult {
        tool_result(async { client(&self.ctx).await?.get(&a.calendar_id, &a.event_id).await.map_err(|e| e.to_string()) }.await)
    }

    #[tool(description = "The events of a window as an iCalendar (RFC 5545) document.", annotations(read_only_hint = true, destructive_hint = false))]
    async fn download_ical(&self, Parameters(a): Parameters<IcalArgs>) -> CallToolResult {
        tool_result(
            async {
                let evs = events_around(&self.ctx, &a.calendar_id, a.past_days, a.future_days, None).await?;
                Ok(json!({ "events": evs.len(), "ical": render_ical(&evs, Utc::now()) }))
            }
            .await,
        )
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Google Calendar, read-only: events around today, one event, iCal export.")
    }
}

/// The CLI's own subcommands (beside auth, token, status, serve).
pub mod commands {
    use super::*;
    use crate::output::{render, write_atomic, Format};

    fn date(s: &str) -> Result<NaiveDate, String> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| format!("{s:?} is not a YYYY-MM-DD date"))
    }

    #[derive(Debug, clap::Subcommand)]
    pub enum Command {
        #[command(flatten)]
        Common(crate::cli::Common),
        /// Write the window's events to an Org file (regenerated each time).
        Sync {
            #[arg(short, long)]
            output: std::path::PathBuf,
            #[arg(long, default_value_t = 90)]
            past_days: i64,
            #[arg(long, default_value_t = 90)]
            future_days: i64,
            /// YYYY-MM-DD; with --until, replaces the day counts.
            #[arg(long, requires = "until")]
            since: Option<String>,
            /// YYYY-MM-DD, inclusive.
            #[arg(long, requires = "since")]
            until: Option<String>,
            #[arg(long, default_value = "primary")]
            calendar_id: String,
            /// IANA zone for the Org timestamps; default $TZ, then /etc/localtime.
            #[arg(long)]
            timezone: Option<String>,
        },
        List {
            #[arg(long, default_value = "primary")]
            calendar_id: String,
            #[arg(long, default_value_t = 90)]
            past_days: i64,
            #[arg(long, default_value_t = 90)]
            future_days: i64,
            #[arg(long, default_value_t = 50)]
            max_results: usize,
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
        /// iCalendar to --output FILE or stdout.
        Download {
            #[arg(long)]
            output: Option<std::path::PathBuf>,
            #[arg(long, default_value = "primary")]
            calendar_id: String,
            #[arg(long, default_value_t = 90)]
            past_days: i64,
            #[arg(long, default_value_t = 90)]
            future_days: i64,
        },
    }

    fn line(e: &Event, tz: Tz) -> String {
        let when = match (e.start_date, e.start_utc) {
            (Some(d), _) => d.format("%Y-%m-%d (all day)").to_string(),
            (_, Some(t)) => t.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string(),
            _ => String::new(),
        };
        format!("  {when}  {} ({})", e.summary, e.id)
    }

    pub async fn run(ctx: &Context, command: Command) -> Result<String, String> {
        match command {
            Command::Common(_) => unreachable!("handled by cli::common"),
            Command::Sync { output, past_days, future_days, since, until, calendar_id, timezone } => {
                let (tz, today) = tz_now(timezone.as_deref())?;
                let (from, to) = match (since, until) {
                    (Some(s), Some(u)) => range(date(&s)?, date(&u)?, tz)?,
                    _ => window(today, tz, past_days, future_days)?,
                };
                let evs = client(ctx).await?.events(&calendar_id, from, to).await.map_err(|e| e.to_string())?;
                write_atomic(&output, &render_org(&evs, tz))?;
                Ok(format!("[+] wrote {} event(s) to {}", evs.len(), output.display()))
            }
            Command::List { calendar_id, past_days, future_days, max_results, format } => {
                let (tz, _) = tz_now(None)?;
                let mut evs = events_around(ctx, &calendar_id, past_days, future_days, None).await?;
                evs.truncate(max_results);
                Ok(render(&evs, format, |evs| {
                    let mut out = vec![format!("{} event(s):", evs.len())];
                    out.extend(evs.iter().map(|e| line(e, tz)));
                    out.join("\n")
                }))
            }
            Command::Get { event_id, calendar_id, format } => {
                let (tz, _) = tz_now(None)?;
                let e = client(ctx).await?.get(&calendar_id, &event_id).await.map_err(|e| e.to_string())?;
                Ok(render(&e, format, |e| line(e, tz).trim().to_string()))
            }
            Command::Download { output, calendar_id, past_days, future_days } => {
                let evs = events_around(ctx, &calendar_id, past_days, future_days, None).await?;
                let ics = render_ical(&evs, Utc::now());
                match output {
                    Some(path) => {
                        write_atomic(&path, &ics)?;
                        Ok(format!("[+] wrote {} event(s) to {}", evs.len(), path.display()))
                    }
                    None => Ok(ics.trim_end().to_string()),
                }
            }
        }
    }
}
