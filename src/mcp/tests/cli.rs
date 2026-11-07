use mcp::{
    cli::{parse, Command, ConfigCommand, Selection},
    config::Transport,
    edit::{Kind, ProfileSpec, ServerSpec},
};

fn p(args: &str) -> Result<Command, mcp::cli::UsageError> {
    parse(&args.split_whitespace().map(String::from).collect::<Vec<_>>())
}

#[test]
fn proxy_help_and_version() {
    assert_eq!(p("proxy").unwrap(), Command::Proxy);
    assert_eq!(p("--help").unwrap(), Command::Help);
    assert_eq!(p("").unwrap(), Command::Help);
    assert_eq!(p("--version").unwrap(), Command::Version);
}

#[test]
fn client_with_a_profile() {
    assert_eq!(p("client --profile coding").unwrap(), Command::Client { selection: Selection::Profile("coding".into()), project: false });
    assert_eq!(p("client --project --profile coding").unwrap(), Command::Client { selection: Selection::Profile("coding".into()), project: true });
}

#[test]
fn client_inline_flags_split_commas_and_repeat() {
    let c = p("client --server slack --server gtasks --include slack:search_*,list_* --include get_task --exclude post --toon --timeout 30").unwrap();
    assert_eq!(
        c,
        Command::Client {
            selection: Selection::Inline {
                servers: vec!["slack".into(), "gtasks".into()],
                include: vec!["slack:search_*".into(), "list_*".into(), "get_task".into()],
                exclude: vec!["post".into()],
                toon: true,
                timeout_secs: Some(30),
            },
            project: false,
        }
    );
}

#[test]
fn client_needs_exactly_one_selection() {
    assert!(p("client").is_err());
    assert!(p("client --profile a --server b").is_err());
    assert!(p("client --profile a --toon").is_err(), "inline flags only go with --server");
    assert!(p("client --include x").is_err());
}

#[test]
fn bad_values_and_unknown_flags_are_usage_errors() {
    assert!(p("client --server s --timeout soon").unwrap_err().0.contains("--timeout"));
    assert!(p("client --profile").unwrap_err().0.contains("--profile"));
    assert!(p("client --profile a --frobnicate").unwrap_err().0.contains("--frobnicate"));
    assert!(p("teleport").unwrap_err().0.contains("teleport"));
    assert!(p("proxy --extra").is_err());
}

#[test]
fn sidecar_takes_everything_after_the_double_dash() {
    assert_eq!(
        p("sidecar --name web -- playwright-mcp --headless --port 0").unwrap(),
        Command::Sidecar { name: "web".into(), argv: vec!["playwright-mcp".into(), "--headless".into(), "--port".into(), "0".into()] }
    );
    assert!(p("sidecar --name web").is_err());
    assert!(p("sidecar -- cmd").is_err());
}

#[test]
fn doctor_flags() {
    assert_eq!(p("doctor").unwrap(), Command::Doctor { profile: None, json: false });
    assert_eq!(p("doctor --json --profile c").unwrap(), Command::Doctor { profile: Some("c".into()), json: true });
}

#[test]
fn usage_is_in_every_error() {
    assert!(p("nope").unwrap_err().to_string().contains("mcp client --profile P"));
}

#[test]
fn sidecar_and_doctor_reject_what_they_do_not_know() {
    assert!(p("sidecar --name web --").unwrap_err().0.contains("command after --"));
    assert!(p("sidecar --name web --port 3 -- cmd").unwrap_err().0.contains("--port"));
    assert!(p("doctor --verbose").unwrap_err().0.contains("--verbose"));
}

#[test]
fn config_add_mcp_with_every_flag() {
    assert_eq!(
        p("config add-mcp scratch --command my-mcp --prefix s --include a,b --exclude c --describe a=Does_A --timeout 9 -- --stdio -v").unwrap(),
        Command::Config(ConfigCommand::AddMcp {
            name: "scratch".into(),
            spec: ServerSpec {
                transport: Some(Transport::Stdio { command: "my-mcp".into(), args: vec!["--stdio".into(), "-v".into()], env: Default::default() }),
                prefix: Some("s".into()),
                include: vec!["a".into(), "b".into()],
                exclude: vec!["c".into()],
                describe: vec![("a".into(), "Does_A".into())],
                timeout_secs: Some(9),
            },
        })
    );
    assert_eq!(
        p("config add-mcp r --unix /run/r.sock").unwrap(),
        Command::Config(ConfigCommand::AddMcp { name: "r".into(), spec: ServerSpec { transport: Some(Transport::Unix("/run/r.sock".into())), ..Default::default() } })
    );
    assert!(p("config add-mcp r --unix /a --command b").is_err(), "one transport");
    assert!(p("config add-mcp r --unix /a -- x").is_err(), "args only follow --command");
    assert!(p("config add-mcp r --describe nodelimiter").unwrap_err().0.contains("tool=TEXT"));
    assert!(p("config add-mcp").is_err());
    assert!(p("config add-mcp --unix /a").is_err(), "a name comes first");
}

#[test]
fn config_add_profile_remove_show_and_tui() {
    assert_eq!(
        p("config add-profile comms --server slack --server gtasks --include slack:search_* --toon --timeout 5").unwrap(),
        Command::Config(ConfigCommand::AddProfile {
            name: "comms".into(),
            spec: ProfileSpec {
                servers: vec!["slack".into(), "gtasks".into()],
                include: vec!["slack:search_*".into()],
                exclude: vec![],
                toon: true,
                timeout_secs: Some(5),
            },
        })
    );
    assert_eq!(p("config remove-mcp s").unwrap(), Command::Config(ConfigCommand::Remove { kind: Kind::Server, name: "s".into() }));
    assert_eq!(p("config remove-profile p").unwrap(), Command::Config(ConfigCommand::Remove { kind: Kind::Profile, name: "p".into() }));
    assert_eq!(p("config show").unwrap(), Command::Config(ConfigCommand::Show { origins: false }));
    assert_eq!(p("config show --origin").unwrap(), Command::Config(ConfigCommand::Show { origins: true }));
    assert_eq!(p("config-tui").unwrap(), Command::ConfigTui);
    assert!(p("config").is_err() && p("config frob").is_err() && p("config remove-mcp").is_err());
    assert!(p("config remove-mcp a b").is_err() && p("config show --all").is_err());
}
