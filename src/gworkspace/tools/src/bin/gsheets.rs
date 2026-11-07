//! gsheets: Google Sheets CLI and MCP server.
use std::process::ExitCode;

use clap::Parser;
use gworkspace::{cli, sheets::{commands::{run, Command}, Server}};

#[derive(Parser)]
#[command(name = "gsheets", version, about = "Google Sheets CLI and MCP server (gworkspace). Ranges are A1 notation.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = match cli::context("gsheets") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match cli.command {
        Command::Common(common) => cli::common(ctx, common, Server::new).await,
        other => cli::finish("gsheets", run(&ctx, other).await),
    }
}
