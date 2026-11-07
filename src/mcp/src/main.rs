use std::process::ExitCode;

use mcp::{app, cli};

#[tokio::main]
async fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[-] mcp: {e}");
            return ExitCode::from(2);
        }
    };
    let env: app::Env = std::env::vars().collect();
    let result = match command {
        cli::Command::Help => {
            println!("{}", cli::USAGE);
            Ok(true)
        }
        cli::Command::Version => {
            println!("mcp {}", env!("CARGO_PKG_VERSION"));
            Ok(true)
        }
        cli::Command::Proxy => app::proxy(&env).await.map(|()| true),
        cli::Command::Client { selection, project } => app::client(&env, &selection, project).await.map(|()| true),
        cli::Command::Sidecar { name, argv } => app::sidecar(&env, &name, &argv).await.map(|()| true),
        cli::Command::Doctor { profile, json } => app::doctor(&env, profile.as_deref(), json).await,
        cli::Command::Config(command) => app::config(&env, &command).map(|()| true),
        cli::Command::ConfigTui => app::config_tui(&env).await.map(|()| true),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("[-] mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
