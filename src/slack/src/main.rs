//! slack: the CLI (send with rich options, edits, drafts, schedules) and `slack serve` (MCP).
use std::{process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use slack::{actions, client::API, mcp::Server, setup::{load, new_msg_id, Setup}, spec::{Attach, Schedule, Spec, Table}};

#[derive(Parser)]
#[command(name = "slack", version, about = "Slack CLI and MCP server: sending goes through the slack.yaml policy (live, draft or deny).")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Send a message (flags override --from-yaml/--from-json fields).
    Send(Box<SendArgs>),
    /// Edit a message by its link.
    Edit {
        link: String,
        #[arg(long)]
        markdown: Option<std::path::PathBuf>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Delete a message by its link.
    Delete { link: String },
    /// React to a message: slack react LINK :emoji:
    React { link: String, emoji: String },
    /// Where a target goes and what the policy says.
    Resolve { target: String },
    /// Composer drafts: list | delete ID CHANNEL
    Drafts {
        #[arg(default_value = "list")]
        action: String,
        id: Option<String>,
        channel: Option<String>,
    },
    /// Scheduled messages: list | delete ID CHANNEL
    Scheduled {
        #[arg(default_value = "list")]
        action: String,
        id: Option<String>,
        channel: Option<String>,
    },
    /// Configuration and token health.
    Status,
    /// MCP server: stdio, --socket PATH or --systemd-socket.
    Serve {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        listen: Vec<String>,
    },
}

#[derive(clap::Args)]
struct SendArgs {
    /// me, @user, #channel, an id.
    #[arg(long)]
    to: Option<String>,
    #[arg(long, conflicts_with_all = ["markdown", "blocks"])]
    text: Option<String>,
    /// A Markdown file.
    #[arg(long, conflicts_with = "blocks")]
    markdown: Option<String>,
    /// A Block Kit JSON file.
    #[arg(long)]
    blocks: Option<std::path::PathBuf>,
    #[arg(long)]
    attach: Vec<String>,
    #[arg(long = "as-image")]
    as_image: Vec<String>,
    #[arg(long = "as-table")]
    as_table: Vec<String>,
    /// LANG FILE
    #[arg(long, num_args = 2, value_names = ["LANG", "FILE"])]
    snippet: Vec<String>,
    /// Text for the shared voice, attached as audio.
    #[arg(long)]
    say: Option<String>,
    #[arg(long = "no-preview")]
    no_preview: bool,
    #[arg(long = "no-link-preview")]
    no_link_preview: bool,
    #[arg(long = "no-media-preview")]
    no_media_preview: bool,
    /// A message link or ts.
    #[arg(long)]
    thread: Option<String>,
    #[arg(long, requires = "thread")]
    broadcast: bool,
    /// RFC 3339, or YYYY-MM-DD HH:MM in --tz.
    #[arg(long)]
    schedule: Option<String>,
    #[arg(long, requires = "schedule")]
    tz: Option<String>,
    /// Ask for a draft (never more than the policy allows).
    #[arg(long)]
    draft: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, conflicts_with = "from_json")]
    from_yaml: Option<std::path::PathBuf>,
    #[arg(long)]
    from_json: Option<std::path::PathBuf>,
}

fn read(path: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The spec: the file's, then every flag that is given on top (pure apart from reading files).
fn spec_of(a: SendArgs) -> Result<(Spec, bool), String> {
    let mut spec = match (&a.from_yaml, &a.from_json) {
        (Some(p), _) => slack::spec::from_yaml(&read(p)?)?,
        (_, Some(p)) => slack::spec::from_json(&read(p)?)?,
        _ => Spec::default(),
    };
    if let Some(to) = a.to {
        spec.to = to;
    }
    if spec.to.is_empty() {
        return Err("--to is required (or `to:` in the file)".into());
    }
    if let Some(t) = a.text {
        spec.text = Some(t);
    }
    if let Some(m) = a.markdown {
        spec.markdown_file = Some(m);
    }
    if let Some(b) = a.blocks {
        spec.blocks = Some(serde_json::from_str(&read(&b)?).map_err(|e| format!("{}: {e}", b.display()))?);
    }
    spec.attach.extend(a.attach.into_iter().map(|path| Attach { path, kind: None, alt: None, title: None, lang: None }));
    spec.attach.extend(a.as_image.into_iter().map(|path| Attach { path, kind: Some("image".into()), alt: None, title: None, lang: None }));
    for pair in a.snippet.chunks(2) {
        spec.attach.push(Attach { path: pair[1].clone(), kind: Some("snippet".into()), alt: None, title: None, lang: Some(pair[0].clone()) });
    }
    spec.tables.extend(a.as_table.into_iter().map(|csv| Table { csv }));
    if a.say.is_some() {
        spec.say = a.say;
    }
    if a.no_preview || a.no_link_preview {
        spec.unfurl.links = Some(false);
    }
    if a.no_preview || a.no_media_preview {
        spec.unfurl.media = Some(false);
    }
    if a.thread.is_some() {
        spec.thread = a.thread;
    }
    spec.broadcast |= a.broadcast;
    if let Some(at) = a.schedule {
        spec.schedule = Some(Schedule { at, tz: a.tz });
    }
    if a.draft {
        spec.mode = Some("draft".into());
    }
    Ok((spec, a.dry_run))
}

fn pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).expect("json")
}

async fn run(setup: &Setup, command: Command) -> Result<String, String> {
    if let Command::Status = command {
        return Ok(pretty(&match &setup.slack {
            Ok(s) => serde_json::json!({ "configured": true, "session_token": s.client.is_session(), "policy_rules": setup.config.policy.rules.len() }),
            Err(why) => serde_json::json!({ "configured": false, "reason": why }),
        }));
    }
    if let Command::Send(a) = command {
        let (spec, dry) = spec_of(*a)?;
        slack::spec::validate(&spec)?;
        let s = setup.slack.as_ref().map_err(Clone::clone)?;
        return actions::send_spec(s, &setup.config, &spec, dry, &setup.tts, &new_msg_id()).await.map(|v| pretty(&v));
    }
    let s = setup.slack.as_ref().map_err(Clone::clone)?;
    let e = |err: slack::client::SlackError| err.to_string();
    let v = match command {
        Command::Send(_) | Command::Status | Command::Serve { .. } => unreachable!("handled before"),
        Command::Edit { link, markdown, text } => {
            let md = markdown.as_deref().map(read).transpose()?;
            actions::update(s, &setup.config, &link, md.as_deref(), text.as_deref()).await?
        }
        Command::Delete { link } => actions::delete(s, &setup.config, &link).await?,
        Command::React { link, emoji } => actions::react(s, &setup.config, &link, &emoji).await?,
        Command::Resolve { target } => {
            let dest = slack::send::resolve(s, &slack::target::parse(&target)?).await?;
            let (action, rule) = slack::policy::decide(&setup.config.policy, &dest);
            serde_json::json!({ "channel": dest.channel_id, "name": dest.name, "self_dm": dest.self_dm, "action": action, "rule": rule })
        }
        Command::Drafts { action, id, channel } => match (action.as_str(), id, channel) {
            ("list", _, _) => s.client.call("drafts.list", &[]).await.map_err(e)?,
            ("delete", Some(id), Some(ch)) => actions::draft_delete(s, &setup.config, &id, &ch).await?,
            _ => return Err("drafts list | drafts delete ID CHANNEL".into()),
        },
        Command::Scheduled { action, id, channel } => match (action.as_str(), id, channel) {
            ("list", _, _) => s.client.call("chat.scheduledMessages.list", &[]).await.map_err(e)?,
            ("delete", Some(id), Some(ch)) => actions::schedule_delete(s, &setup.config, &id, &ch).await?,
            _ => return Err("scheduled list | scheduled delete ID CHANNEL".into()),
        },
    };
    Ok(pretty(&v))
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let base = env.get("SLACK_API_BASE").cloned().unwrap_or_else(|| API.to_string());
    let setup = match load(&env, &base) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("[-] slack: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Command::Serve { listen } = &cli.command {
        let listen = match mcp_serve::parse_listen(listen) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[-] slack serve: {e}");
                return ExitCode::from(2);
            }
        };
        return match mcp_serve::serve(&listen, move || Server::new(setup.clone()), Default::default()).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[-] slack serve: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match run(&setup, cli.command).await {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[-] slack: {e}");
            ExitCode::FAILURE
        }
    }
}
