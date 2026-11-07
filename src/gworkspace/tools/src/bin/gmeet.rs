//! gmeet: Google Meet CLI and MCP server (read-only).
use std::process::ExitCode;

use clap::Parser;
use gworkspace::{cli, meet::{commands::{run, Command}, Server}};

#[derive(Parser)]
#[command(name = "gmeet", version, about = "Google Meet CLI and MCP server (gworkspace), read-only.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = match cli::context("gmeet") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match cli.command {
        Command::Common(common) => cli::common(ctx, common, Server::new).await,
        other => cli::finish("gmeet", run(&ctx, other).await),
    }
}
