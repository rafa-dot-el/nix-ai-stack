//! Command-line parsing for `mcp`. Pure: argv in, [`Command`] out.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Profile(String),
    Inline { servers: Vec<String>, include: Vec<String>, exclude: Vec<String>, toon: bool, timeout_secs: Option<u64> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    AddMcp { name: String, spec: crate::edit::ServerSpec },
    AddProfile { name: String, spec: crate::edit::ProfileSpec },
    Remove { kind: crate::edit::Kind, name: String },
    Show { origins: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Config(ConfigCommand),
    ConfigTui,
    Proxy,
    Client { selection: Selection, project: bool },
    Sidecar { name: String, argv: Vec<String> },
    Doctor { profile: Option<String>, json: bool },
    Help,
    Version,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0}\n\n{USAGE}")]
pub struct UsageError(pub String);

pub const USAGE: &str = "usage:
  mcp proxy
  mcp client --profile P [--project]
  mcp client --server S [--server S]... [--include a,b] [--exclude c] [--toon] [--timeout SECS] [--project]
  mcp sidecar --name N -- CMD [ARGS...]
  mcp doctor [--profile P] [--json]
  mcp config add-mcp NAME [--unix PATH | --command CMD] [--prefix P] [--include a,b] [--exclude c]
                          [--describe tool=TEXT] [--timeout SECS] [-- ARGS...]
  mcp config add-profile NAME --server S [--server S]... [--include ..] [--exclude ..] [--toon] [--timeout SECS]
  mcp config remove-mcp NAME | remove-profile NAME | show [--origin]
  mcp config-tui
  mcp --help | --version";

fn usage(msg: impl Into<String>) -> UsageError {
    UsageError(msg.into())
}

/// Walks the flags of one subcommand.
struct Flags<'a> {
    args: std::slice::Iter<'a, String>,
}

impl<'a> Flags<'a> {
    fn value(&mut self, flag: &str) -> Result<String, UsageError> {
        match self.args.next() {
            Some(v) if !v.starts_with("--") => Ok(v.clone()),
            _ => Err(usage(format!("{flag} needs a value"))),
        }
    }
}

fn split(value: &str) -> impl Iterator<Item = String> + '_ {
    value.split(',').filter(|s| !s.is_empty()).map(str::to_string)
}

fn client(flags: &mut Flags<'_>) -> Result<Command, UsageError> {
    let (mut profile, mut project, mut toon, mut timeout_secs) = (None, false, false, None);
    let (mut servers, mut include, mut exclude) = (Vec::new(), Vec::new(), Vec::new());
    while let Some(flag) = flags.args.next() {
        match flag.as_str() {
            "--profile" => profile = Some(flags.value(flag)?),
            "--project" => project = true,
            "--server" => servers.push(flags.value(flag)?),
            "--include" => include.extend(split(&flags.value(flag)?)),
            "--exclude" => exclude.extend(split(&flags.value(flag)?)),
            "--toon" => toon = true,
            "--timeout" => {
                let v = flags.value(flag)?;
                timeout_secs = Some(v.parse().map_err(|_| usage(format!("--timeout wants seconds, not {v:?}")))?);
            }
            other => return Err(usage(format!("unknown flag {other:?} for `mcp client`"))),
        }
    }
    let inline_flags = !include.is_empty() || !exclude.is_empty() || toon || timeout_secs.is_some();
    let selection = match (profile, servers.is_empty()) {
        (Some(_), false) => return Err(usage("--profile and --server are exclusive")),
        (Some(_), true) if inline_flags => {
            return Err(usage("--include/--exclude/--toon/--timeout go with --server; a profile sets them in mcp.yaml"))
        }
        (Some(p), true) => Selection::Profile(p),
        (None, false) => Selection::Inline { servers, include, exclude, toon, timeout_secs },
        (None, true) => return Err(usage("`mcp client` needs --profile P or --server S")),
    };
    Ok(Command::Client { selection, project })
}

fn sidecar(flags: &mut Flags<'_>) -> Result<Command, UsageError> {
    let mut name = None;
    while let Some(flag) = flags.args.next() {
        match flag.as_str() {
            "--name" => name = Some(flags.value(flag)?),
            "--" => {
                let argv: Vec<String> = flags.args.by_ref().cloned().collect();
                let name = name.ok_or_else(|| usage("`mcp sidecar` needs --name N"))?;
                if argv.is_empty() {
                    return Err(usage("`mcp sidecar` needs a command after --"));
                }
                return Ok(Command::Sidecar { name, argv });
            }
            other => return Err(usage(format!("unknown flag {other:?} for `mcp sidecar`"))),
        }
    }
    Err(usage("`mcp sidecar` needs -- CMD [ARGS...]"))
}

fn doctor(flags: &mut Flags<'_>) -> Result<Command, UsageError> {
    let (mut profile, mut json) = (None, false);
    while let Some(flag) = flags.args.next() {
        match flag.as_str() {
            "--profile" => profile = Some(flags.value(flag)?),
            "--json" => json = true,
            other => return Err(usage(format!("unknown flag {other:?} for `mcp doctor`"))),
        }
    }
    Ok(Command::Doctor { profile, json })
}

fn timeout(flags: &mut Flags<'_>, flag: &str) -> Result<Option<u64>, UsageError> {
    let v = flags.value(flag)?;
    v.parse().map(Some).map_err(|_| usage(format!("--timeout wants seconds, not {v:?}")))
}

fn name(flags: &mut Flags<'_>, what: &str) -> Result<String, UsageError> {
    match flags.args.next() {
        Some(n) if !n.starts_with('-') => Ok(n.clone()),
        _ => Err(usage(format!("`mcp config {what}` needs a NAME first"))),
    }
}

fn add_mcp(flags: &mut Flags<'_>) -> Result<ConfigCommand, UsageError> {
    use crate::config::Transport;
    let name = name(flags, "add-mcp")?;
    let mut spec = crate::edit::ServerSpec::default();
    let mut command: Option<String> = None;
    while let Some(flag) = flags.args.next() {
        match flag.as_str() {
            "--unix" | "--command" if spec.transport.is_some() || command.is_some() => {
                return Err(usage("give one of --unix or --command"))
            }
            "--unix" => spec.transport = Some(Transport::Unix(flags.value(flag)?)),
            "--command" => command = Some(flags.value(flag)?),
            "--prefix" => spec.prefix = Some(flags.value(flag)?),
            "--include" => spec.include.extend(split(&flags.value(flag)?)),
            "--exclude" => spec.exclude.extend(split(&flags.value(flag)?)),
            "--describe" => {
                let v = flags.value(flag)?;
                let (tool, text) = v.split_once('=').ok_or_else(|| usage(format!("--describe wants tool=TEXT, not {v:?}")))?;
                spec.describe.push((tool.to_string(), text.to_string()));
            }
            "--timeout" => spec.timeout_secs = timeout(flags, flag)?,
            "--" => {
                let Some(cmd) = command.take() else { return Err(usage("arguments after -- only go with --command")) };
                let args = flags.args.by_ref().cloned().collect();
                spec.transport = Some(Transport::Stdio { command: cmd, args, env: Default::default() });
            }
            other => return Err(usage(format!("unknown flag {other:?} for `mcp config add-mcp`"))),
        }
    }
    if let Some(cmd) = command {
        spec.transport = Some(Transport::Stdio { command: cmd, args: Vec::new(), env: Default::default() });
    }
    Ok(ConfigCommand::AddMcp { name, spec })
}

fn add_profile(flags: &mut Flags<'_>) -> Result<ConfigCommand, UsageError> {
    let name = name(flags, "add-profile")?;
    let mut spec = crate::edit::ProfileSpec::default();
    while let Some(flag) = flags.args.next() {
        match flag.as_str() {
            "--server" => spec.servers.push(flags.value(flag)?),
            "--include" => spec.include.extend(split(&flags.value(flag)?)),
            "--exclude" => spec.exclude.extend(split(&flags.value(flag)?)),
            "--toon" => spec.toon = true,
            "--timeout" => spec.timeout_secs = timeout(flags, flag)?,
            other => return Err(usage(format!("unknown flag {other:?} for `mcp config add-profile`"))),
        }
    }
    Ok(ConfigCommand::AddProfile { name, spec })
}

fn config(flags: &mut Flags<'_>) -> Result<Command, UsageError> {
    use crate::edit::Kind;
    let sub = flags.args.next().ok_or_else(|| usage("`mcp config` needs add-mcp, add-profile, remove-mcp, remove-profile or show"))?;
    let command = match sub.as_str() {
        "add-mcp" => return add_mcp(flags).map(Command::Config),
        "add-profile" => return add_profile(flags).map(Command::Config),
        "remove-mcp" => ConfigCommand::Remove { kind: Kind::Server, name: name(flags, "remove-mcp")? },
        "remove-profile" => ConfigCommand::Remove { kind: Kind::Profile, name: name(flags, "remove-profile")? },
        "show" => match flags.args.next().map(String::as_str) {
            None => return Ok(Command::Config(ConfigCommand::Show { origins: false })),
            Some("--origin") => ConfigCommand::Show { origins: true },
            Some(other) => return Err(usage(format!("unknown flag {other:?} for `mcp config show`"))),
        },
        other => return Err(usage(format!("unknown config command {other:?}"))),
    };
    match flags.args.next() {
        Some(extra) => Err(usage(format!("unexpected argument {extra:?}"))),
        None => Ok(Command::Config(command)),
    }
}

/// Parse `argv` without the program name.
pub fn parse(argv: &[String]) -> Result<Command, UsageError> {
    let Some((sub, rest)) = argv.split_first() else { return Ok(Command::Help) };
    let mut flags = Flags { args: rest.iter() };
    let command = match sub.as_str() {
        "--help" | "-h" | "help" => Command::Help,
        "--version" | "-V" => Command::Version,
        "proxy" => Command::Proxy,
        "client" => return client(&mut flags),
        "sidecar" => return sidecar(&mut flags),
        "doctor" => return doctor(&mut flags),
        "config" => return config(&mut flags),
        "config-tui" => Command::ConfigTui,
        other => return Err(usage(format!("unknown command {other:?}"))),
    };
    match flags.args.next() {
        Some(extra) => Err(usage(format!("unexpected argument {extra:?}"))),
        None => Ok(command),
    }
}
