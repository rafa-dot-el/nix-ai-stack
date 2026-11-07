//! gmail: Gmail CLI and MCP server (drafts only; nothing is sent).
use std::process::ExitCode;

use clap::Parser;
use gworkspace::{cli, mail::{commands::{run, Command}, Server}};

#[derive(Parser)]
#[command(name = "gmail", version, about = "Gmail CLI and MCP server (gworkspace): read, export, labels, drafts (never sends).")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = match cli::context("gmail") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match cli.command {
        Command::Common(common) => cli::common(ctx, common, Server::new).await,
        other => cli::finish("gmail", run(&ctx, other).await),
    }
}
