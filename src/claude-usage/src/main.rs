//! claude-usage: Claude Code usage as a table, a format string (polybar) or JSON, and
//! `claude-usage exporter` (Prometheus).
use std::{io::IsTerminal, process::ExitCode, sync::Arc, time::Duration};

use claude_usage::{client::Client, config, creds, exporter, format, table, vars};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "claude-usage", version, about = "Claude Code usage: table, format string (polybar) or JSON; `exporter` serves Prometheus metrics. Reads ~/.claude/.credentials.json, never writes it.")]
struct Cli {
    /// A template with variables, e.g. "{PCT_5H}% | {COUNTDOWN_5H}".
    #[arg(long)]
    format: Option<String>,
    /// polybar, ansi or none (default: ansi on a terminal, none otherwise).
    #[arg(long)]
    color_mode: Option<String>,
    /// The API's JSON.
    #[arg(long)]
    json: bool,
    /// A configuration file instead of ~/.config/rafa/claude-usage.yaml.
    #[arg(long, global = true)]
    config: Option<String>,
    /// The credentials file (default: from the configuration, else ~/.claude/.credentials.json).
    #[arg(long, global = true)]
    credentials: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve Prometheus metrics (bind, port and path from the configuration).
    Exporter,
}

async fn run(cli: Cli) -> Result<(), String> {
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let mut cfg = config::load(&env, cli.config.as_deref())?;
    if let Some(c) = cli.credentials {
        cfg.credentials_path = c;
    }
    let client = Arc::new(Client::new(&cfg.endpoint)?);
    if let Some(Command::Exporter) = cli.command {
        let addr = format!("{}:{}", cfg.exporter.bind, cfg.exporter.port);
        let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| format!("{addr}: {e}"))?;
        eprintln!("[*] claude-usage exporter on http://{}{}", listener.local_addr().map_err(|e| e.to_string())?, cfg.exporter.path);
        let (creds_path, ttl) = (cfg.credentials_path.clone(), Duration::from_secs(cfg.exporter.cache_ttl_secs));
        return exporter::serve(listener, cfg.exporter.path.clone(), move || {
            let (client, creds_path) = (client.clone(), creds_path.clone());
            async move {
                let token = creds::get_valid_token(&creds_path).map_err(|e| e.to_string())?;
                let usage = client.fetch_cached(&token, ttl).await?;
                Ok(exporter::metrics(&usage, &chrono::Utc::now()))
            }
        })
        .await
        .map_err(|e| e.to_string());
    }
    let token = creds::get_valid_token(&cfg.credentials_path).map_err(|e| e.to_string())?;
    let usage = client.fetch(&token).await?;
    let mut v = vars::build(&usage);
    vars::add_creds_vars(&mut v, &cfg.credentials_path);
    let terminal = std::io::stdout().is_terminal();
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&usage).map_err(|e| e.to_string())?);
    } else if let Some(f) = cli.format {
        let mode = match cli.color_mode {
            Some(m) => m.parse().unwrap_or(format::ColorMode::Polybar),
            None if terminal => format::ColorMode::Ansi,
            None => format::ColorMode::None,
        };
        println!("{}", format::expand(&f, &v, mode));
    } else {
        print!("{}", table::table(&usage, &v, terminal));
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[-] claude-usage: {e}");
            ExitCode::FAILURE
        }
    }
}
