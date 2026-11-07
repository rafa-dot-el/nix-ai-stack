//! gcal: Google Calendar CLI and MCP server (read-only).
use std::process::ExitCode;

use clap::Parser;
use gworkspace::{calendar::{commands::{run, Command}, Server}, cli};

#[derive(Parser)]
#[command(name = "gcal", version, about = "Google Calendar CLI and MCP server (gworkspace), read-only; Org sync and iCal export.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = match cli::context("gcal") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match cli.command {
        Command::Common(common) => cli::common(ctx, common, Server::new).await,
        other => cli::finish("gcal", run(&ctx, other).await),
    }
}
