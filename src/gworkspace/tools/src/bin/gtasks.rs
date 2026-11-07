//! gtasks: Google Tasks CLI and MCP server. Lists are named by id or title.
use std::process::ExitCode;

use clap::Parser;
use gworkspace::{cli, tasks::{commands::{run, Command}, Server}};

#[derive(Parser)]
#[command(name = "gtasks", version, about = "Google Tasks CLI and MCP server (gworkspace). --list takes an id or a title.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = match cli::context("gtasks") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match cli.command {
        Command::Common(common) => cli::common(ctx, common, Server::new).await,
        other => cli::finish("gtasks", run(&ctx, other).await),
    }
}
