//! What every gworkspace binary shares: auth, token, status and serve, and how results print.
use std::process::ExitCode;

use gworkspace_auth::{consent_url, COMBINED_SCOPES};

use crate::{config, context::Context};

#[derive(Debug, clap::Subcommand)]
pub enum Common {
    /// OAuth consent: print the URL, take the redirect URL back, store the refresh token.
    Auth {
        #[arg(long)]
        redirect_url: Option<String>,
    },
    /// Print a fresh access token.
    Token,
    /// Check the configuration and that a token can be refreshed.
    Status,
    /// MCP server: stdio, --socket PATH or --systemd-socket.
    Serve {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        listen: Vec<String>,
    },
}

/// The context from ~/.config/rafa/gworkspace.yaml, or the exit for a broken file.
pub fn context(bin: &'static str) -> Result<Context, ExitCode> {
    let env = std::env::vars().collect();
    config::load(&env).map(|c| Context::new(bin, c)).map_err(|e| {
        eprintln!("[-] {bin}: {e}");
        ExitCode::FAILURE
    })
}

/// Print a command's outcome: the text on stdout, or `[-] bin: error` on stderr and exit 1.
pub fn finish(bin: &str, outcome: Result<String, String>) -> ExitCode {
    match outcome {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[-] {bin}: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn auth(ctx: &Context, redirect_url: Option<String>) -> Result<String, String> {
    let auth = ctx.auth()?;
    let client = auth.client().map_err(|e| e.to_string())?;
    println!("Open this URL, consent, then paste the full redirect URL:\n\n{}\n", consent_url(&client, COMBINED_SCOPES));
    let redirect = match redirect_url {
        Some(u) => u,
        None => {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() {
                return Err("no terminal: pass --redirect-url".into());
            }
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
            line
        }
    };
    let token = auth.exchange(&redirect).await.map_err(|e| e.to_string())?;
    Ok(match auth.config.store_refresh_token {
        Some(_) => "[+] refresh token stored".to_string(),
        None => format!("[*] no auth.store_refresh_token configured; store this refresh token yourself:\n{}", token.refresh_token.unwrap_or_default()),
    })
}

/// Run a shared subcommand; `server` builds one MCP handler per connection.
pub async fn common<H, F>(ctx: Context, command: Common, server: F) -> ExitCode
where
    H: rmcp::ServerHandler,
    F: Fn(Context) -> H + Send + Sync + 'static,
{
    let bin = ctx.bin;
    match command {
        Common::Auth { redirect_url } => finish(bin, auth(&ctx, redirect_url).await),
        Common::Token => finish(bin, match ctx.auth() {
            Ok(a) => a.access_token().await.map_err(|e| e.to_string()),
            Err(e) => Err(e),
        }),
        Common::Status => finish(bin, match ctx.auth() {
            Ok(a) => a.access_token().await.map(|_| format!("[+] {bin}: configured, access token refreshed")).map_err(|e| e.to_string()),
            Err(e) => Err(e),
        }),
        Common::Serve { listen } => {
            let listen = match mcp_serve::parse_listen(&listen) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[-] {bin} serve: {e}");
                    return ExitCode::from(2);
                }
            };
            if matches!(ctx.auth, Ok(None)) {
                eprintln!("[*] {bin} serve: not configured; tools will say how to configure");
            }
            match mcp_serve::serve(&listen, move || server(ctx.clone()), Default::default()).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("[-] {bin} serve: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// The JSON a tool returns, or its error text, as a tool result.
pub fn tool_result<T: serde::Serialize>(outcome: Result<T, String>) -> rmcp::model::CallToolResult {
    use rmcp::model::{CallToolResult, ContentBlock};
    match outcome {
        Ok(v) => CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(&v).expect("serialisable"))]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
    }
}
