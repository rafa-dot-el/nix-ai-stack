//! gdocs: Google Docs CLI and MCP server.
use std::process::ExitCode;

use clap::Parser;
use gworkspace::{cli, docs::{commands::{run, Command}, Server}};

#[derive(Parser)]
#[command(name = "gdocs", version, about = "Google Docs CLI and MCP server (gworkspace).")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = match cli::context("gdocs") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match cli.command {
        Command::Common(common) => cli::common(ctx, common, Server::new).await,
        other => cli::finish("gdocs", run(&ctx, other).await),
    }
}
