//! tmpmail: disposable GuerrillaMail inboxes (CLI) and `tmpmail serve` (MCP).
use std::{process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use tmpmail::{format::*, mcp::Server, ops, setup::load};

#[derive(Parser)]
#[command(name = "tmpmail", version, about = "Disposable GuerrillaMail inboxes: no signup, no key. Session tokens stay in $XDG_STATE_HOME/rafa/tmpmail/ (0600).")]
struct Cli {
    /// text, json or yaml.
    #[arg(long, global = true, default_value = "text")]
    format: OutputFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a random inbox and save it.
    NewIdentity,
    /// The saved inboxes.
    ListIdentities,
    /// Messages in an inbox.
    ListEmails {
        #[arg(long)]
        email: Option<String>,
    },
    /// One message in full.
    GetEmail {
        #[arg(long)]
        id: String,
        #[arg(long)]
        email: Option<String>,
        /// The raw HTML body.
        #[arg(long)]
        html: bool,
    },
    /// Destroy an inbox server-side and forget it.
    ForgetIdentity {
        #[arg(long)]
        email: Option<String>,
    },
    /// MCP server: stdio, --socket PATH or --systemd-socket.
    #[command(alias = "serve-mcp")]
    Serve {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        listen: Vec<String>,
    },
}

fn text(command: &Command, v: &serde_json::Value) -> String {
    let s = |k: &str, x: &serde_json::Value| x[k].as_str().unwrap_or("").to_string();
    match command {
        Command::NewIdentity => format!("{}\n", s("email", v)),
        Command::ListIdentities => identity_list_text(
            &v.as_array().into_iter().flatten().map(|i| IdentityView { email: s("email", i), created_at: i["created_at"].as_u64().unwrap_or(0) }).collect::<Vec<_>>(),
        ),
        Command::ListEmails { .. } => message_list_text(
            &v.as_array()
                .into_iter()
                .flatten()
                .map(|m| MessageView { id: s("id", m), from: s("from", m), subject: s("subject", m), date: s("date", m), read: m["read"].as_bool().unwrap_or(false), excerpt: s("excerpt", m) })
                .collect::<Vec<_>>(),
        ),
        Command::GetEmail { .. } => {
            let d = MessageDetailView { id: s("id", v), from: s("from", v), subject: s("subject", v), date: s("date", v), body: s("body", v) };
            message_detail_text(&d, &d.body)
        }
        Command::ForgetIdentity { .. } => format!("forgot {} (server acknowledged: {})\n", s("email", v), v["server_acknowledged"]),
        Command::Serve { .. } => String::new(),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let setup = match load(&env) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("[-] tmpmail: {e}");
            return ExitCode::FAILURE;
        }
    };
    let out = match &cli.command {
        Command::Serve { listen } => {
            let listen = match mcp_serve::parse_listen(listen) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[-] tmpmail serve: {e}");
                    return ExitCode::from(2);
                }
            };
            return match mcp_serve::serve(&listen, move || Server::new(setup.clone()), Default::default()).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("[-] tmpmail serve: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Command::NewIdentity => ops::new_identity(&setup, ops::now()).await,
        Command::ListIdentities => ops::list_identities(&setup).await,
        Command::ListEmails { email } => ops::list_emails(&setup, email.as_deref()).await,
        Command::GetEmail { id, email, html } => ops::get_email(&setup, id, email.as_deref(), *html).await,
        Command::ForgetIdentity { email } => ops::forget_identity(&setup, email.as_deref()).await,
    };
    match out.and_then(|v| render(cli.format, &v, || text(&cli.command, &v))) {
        Ok(s) => {
            print!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[-] tmpmail: {e}");
            ExitCode::FAILURE
        }
    }
}
