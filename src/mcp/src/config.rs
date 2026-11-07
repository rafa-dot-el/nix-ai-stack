//! The typed view of `~/.config/rafa/mcp.yaml` (merged by rafa-config). Pure.
use std::collections::BTreeMap;

use mcp_filter::{Annotations, Matcher, Rules};
use rafa_config::Value;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum McpConfigError {
    #[error("{path}: {reason}")]
    Invalid { path: String, reason: String },
    #[error(transparent)]
    Filter(#[from] mcp_filter::FilterError),
}

/// How the proxy reaches a server that is not in the pool directory (non-Nix `add-mcp`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    Unix(String),
    Stdio { command: String, args: Vec<String>, env: BTreeMap<String, String> },
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Defaults to the server name.
    pub prefix: Option<String>,
    pub rules: Rules,
    pub timeout_secs: Option<u64>,
    /// `None`: discovered in the pool directory under its name.
    pub transport: Option<Transport>,
}

#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub servers: BTreeMap<String, Rules>,
    pub toon: bool,
    pub timeout_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyConfig {
    /// As configured (already expanded); `None`: [`DEFAULT_SOCKET`], resolved when used.
    pub socket: Option<String>,
    /// As configured; `None`: [`DEFAULT_POOL`], resolved when used.
    pub pool: Option<String>,
    pub separator: String,
}

impl ProxyConfig {
    pub fn socket_path(&self, env: &BTreeMap<String, String>) -> Result<std::path::PathBuf, String> {
        resolve(self.socket.as_deref(), DEFAULT_SOCKET, "proxy.socket", env)
    }

    pub fn pool_path(&self, env: &BTreeMap<String, String>) -> Result<std::path::PathBuf, String> {
        resolve(self.pool.as_deref(), DEFAULT_POOL, "proxy.pool", env)
    }
}

/// A configured path, or the default expanded now: a subcommand that never uses a path never
/// needs the variables of its default (e.g. `mcp client` with an explicit socket).
fn resolve(set: Option<&str>, default: &str, key: &str, env: &BTreeMap<String, String>) -> Result<std::path::PathBuf, String> {
    if let Some(path) = set {
        return Ok(path.into());
    }
    match rafa_config::expand(&Value::String(default.to_string()), env, "defaults") {
        Ok(Value::String(path)) => Ok(path.into()),
        Ok(_) => unreachable!("a string expands to a string"),
        Err(e) => Err(format!("{key} is not set and its default {default} cannot be used: {e}")),
    }
}

#[derive(Clone, Debug)]
pub struct McpConfig {
    pub proxy: ProxyConfig,
    pub servers: BTreeMap<String, ServerConfig>,
    pub profiles: BTreeMap<String, Profile>,
}

/// Defaults when `proxy:` leaves a key out, expanded only by the subcommand that needs them.
pub const DEFAULT_SOCKET: &str = "${XDG_RUNTIME_DIR}/rafa/mcp/proxy.sock";
pub const DEFAULT_POOL: &str = "${XDG_RUNTIME_DIR}/rafa/mcp/servers";
pub const DEFAULT_SEPARATOR: &str = "-";

fn invalid(path: &str, reason: impl Into<String>) -> McpConfigError {
    McpConfigError::Invalid { path: path.to_string(), reason: reason.into() }
}

fn mapping<'a>(value: &'a Value, path: &str) -> Result<Option<&'a rafa_config::Value>, McpConfigError> {
    match value {
        Value::Null => Ok(None),
        Value::Mapping(_) => Ok(Some(value)),
        _ => Err(invalid(path, "expected a mapping")),
    }
}

fn str_at(value: &Value, key: &str, path: &str) -> Result<Option<String>, McpConfigError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(invalid(&format!("{path}.{key}"), "expected a string")),
    }
}

fn string_map(value: Option<&Value>, path: &str) -> Result<BTreeMap<String, String>, McpConfigError> {
    let Some(value) = value else { return Ok(BTreeMap::new()) };
    let map = value.as_mapping().ok_or_else(|| invalid(path, "expected a mapping of strings"))?;
    map.iter()
        .map(|(k, v)| match (k.as_str(), v.as_str()) {
            (Some(k), Some(v)) => Ok((k.to_string(), v.to_string())),
            _ => Err(invalid(path, "expected a mapping of strings")),
        })
        .collect()
}

/// `30`, `30s`, `2m`, `1h` -> seconds.
fn seconds(value: Option<&Value>, path: &str) -> Result<Option<u64>, McpConfigError> {
    let text = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Number(n)) => return n.as_u64().map(Some).ok_or_else(|| invalid(path, "expected a duration")),
        Some(Value::String(s)) => s.trim().to_string(),
        Some(_) => return Err(invalid(path, "expected a duration")),
    };
    let (digits, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let n: u64 = digits.parse().map_err(|_| invalid(path, format!("not a duration: {text:?}")))?;
    let factor = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return Err(invalid(path, format!("not a duration: {text:?}"))),
    };
    Ok(Some(n * factor))
}

fn annotations(value: &Value, path: &str) -> Result<Annotations, McpConfigError> {
    let map = value.as_mapping().ok_or_else(|| invalid(path, "expected a mapping of hints"))?;
    let mut out = Annotations::default();
    for (k, v) in map {
        let flag = v.as_bool().ok_or_else(|| invalid(path, "annotation hints are booleans"))?;
        match k.as_str() {
            Some("readOnlyHint") => out.read_only = Some(flag),
            Some("destructiveHint") => out.destructive = Some(flag),
            Some("idempotentHint") => out.idempotent = Some(flag),
            Some("openWorldHint") => out.open_world = Some(flag),
            _ => return Err(invalid(path, format!("unknown annotation hint {k:?}"))),
        }
    }
    Ok(out)
}

fn matchers(value: Option<&Value>, path: &str) -> Result<Vec<Matcher>, McpConfigError> {
    let Some(value) = value else { return Ok(Vec::new()) };
    let items = value.as_sequence().ok_or_else(|| invalid(path, "expected a list of patterns"))?;
    items
        .iter()
        .map(|item| match item {
            Value::String(s) => Ok(Matcher::parse(s)?),
            Value::Mapping(_) => match item.get("annotations") {
                Some(a) => Ok(Matcher::annotations(annotations(a, path)?)),
                None => Err(invalid(path, "a mapping pattern needs `annotations:`")),
            },
            _ => Err(invalid(path, "a pattern is a string or { annotations: {...} }")),
        })
        .collect()
}

/// The rules of one server or profile entry (`include`, `exclude`, `rename`, `describe`).
pub fn rules(value: &Value, path: &str) -> Result<Rules, McpConfigError> {
    Ok(Rules {
        include: matchers(value.get("include"), &format!("{path}.include"))?,
        exclude: matchers(value.get("exclude"), &format!("{path}.exclude"))?,
        rename: string_map(value.get("rename"), &format!("{path}.rename"))?,
        describe: string_map(value.get("describe"), &format!("{path}.describe"))?,
    })
}

fn transport(value: Option<&Value>, path: &str) -> Result<Option<Transport>, McpConfigError> {
    let Some(value) = value else { return Ok(None) };
    if let Some(unix) = value.get("unix").and_then(Value::as_str) {
        return Ok(Some(Transport::Unix(unix.to_string())));
    }
    if let Some(stdio) = value.get("stdio") {
        let command = str_at(stdio, "command", &format!("{path}.stdio"))?
            .ok_or_else(|| invalid(&format!("{path}.stdio"), "needs `command`"))?;
        let args = match stdio.get("args") {
            None => Vec::new(),
            Some(a) => a
                .as_sequence()
                .and_then(|s| s.iter().map(|x| x.as_str().map(str::to_string)).collect::<Option<Vec<_>>>())
                .ok_or_else(|| invalid(&format!("{path}.stdio.args"), "expected a list of strings"))?,
        };
        let env = string_map(stdio.get("env"), &format!("{path}.stdio.env"))?;
        return Ok(Some(Transport::Stdio { command, args, env }));
    }
    Err(invalid(path, "expected { unix: PATH } or { stdio: { command, args } }"))
}

/// Map the merged (and expanded) configuration onto the typed view.
pub fn from_value(value: &Value) -> Result<McpConfig, McpConfigError> {
    let proxy_v = value.get("proxy");
    let proxy = match proxy_v {
        Some(p) => Some(mapping(p, "proxy")?).flatten(),
        None => None,
    };
    let get = |key: &str| -> Result<Option<String>, McpConfigError> {
        match proxy {
            Some(p) => str_at(p, key, "proxy"),
            None => Ok(None),
        }
    };
    let separator = match proxy.and_then(|p| p.get("namespace")) {
        Some(ns) => str_at(ns, "separator", "proxy.namespace")?.unwrap_or_else(|| DEFAULT_SEPARATOR.into()),
        None => DEFAULT_SEPARATOR.into(),
    };
    let proxy = ProxyConfig { socket: get("socket")?, pool: get("pool")?, separator };

    let mut servers = BTreeMap::new();
    if let Some(map) = value.get("servers").and_then(Value::as_mapping) {
        for (name, server) in map {
            let name = name.as_str().ok_or_else(|| invalid("servers", "server names are strings"))?;
            let path = format!("servers.{name}");
            let empty = Value::Mapping(Default::default());
            let server = if server.is_null() { &empty } else { server };
            servers.insert(
                name.to_string(),
                ServerConfig {
                    prefix: str_at(server, "prefix", &path)?,
                    rules: rules(server, &path)?,
                    timeout_secs: seconds(server.get("timeout"), &format!("{path}.timeout"))?,
                    transport: transport(server.get("transport"), &format!("{path}.transport"))?,
                },
            );
        }
    }

    let mut profiles = BTreeMap::new();
    if let Some(map) = value.get("profiles").and_then(Value::as_mapping) {
        for (name, profile) in map {
            let name = name.as_str().ok_or_else(|| invalid("profiles", "profile names are strings"))?;
            let path = format!("profiles.{name}");
            let mut selected = BTreeMap::new();
            if let Some(entries) = profile.get("servers").and_then(Value::as_mapping) {
                for (server, r) in entries {
                    let server = server.as_str().ok_or_else(|| invalid(&path, "server names are strings"))?;
                    let empty = Value::Mapping(Default::default());
                    let r = if r.is_null() { &empty } else { r };
                    selected.insert(server.to_string(), rules(r, &format!("{path}.servers.{server}"))?);
                }
            }
            profiles.insert(
                name.to_string(),
                Profile {
                    servers: selected,
                    toon: profile.get("toon").and_then(Value::as_bool).unwrap_or(false),
                    timeout_secs: seconds(profile.get("timeout"), &format!("{path}.timeout"))?,
                },
            );
        }
    }
    Ok(McpConfig { proxy, servers, profiles })
}

/// Split `--include`/`--exclude` patterns per server: `server:pattern` goes to that server
/// (it must be listed), `re:...` and anything unqualified go to every listed server.
pub fn qualify(servers: &[String], patterns: &[String], flag: &str) -> Result<BTreeMap<String, Vec<String>>, McpConfigError> {
    let mut out: BTreeMap<String, Vec<String>> = servers.iter().map(|s| (s.clone(), Vec::new())).collect();
    for pattern in patterns {
        match pattern.split_once(':') {
            Some((server, pat)) if server != "re" => {
                let list = out
                    .get_mut(server)
                    .ok_or_else(|| invalid(flag, format!("{pattern:?} names server {server:?}, which is not a --server")))?;
                list.push(pat.to_string());
            }
            _ => out.values_mut().for_each(|list| list.push(pattern.clone())),
        }
    }
    Ok(out)
}

/// The profile `mcp client --server A --server B --include x,A:y --exclude z --toon --timeout N`
/// stands for. An unqualified pattern applies to every listed server, `server:pattern` to one.
pub fn inline_profile(
    servers: &[String],
    include: &[String],
    exclude: &[String],
    toon: bool,
    timeout_secs: Option<u64>,
) -> Result<Profile, McpConfigError> {
    let (inc, exc) = (qualify(servers, include, "--include")?, qualify(servers, exclude, "--exclude")?);
    let mut selected = BTreeMap::new();
    for server in servers {
        let compile = |pats: &Vec<String>| pats.iter().map(|p| Matcher::parse(p)).collect::<Result<Vec<_>, _>>();
        let rules = Rules { include: compile(&inc[server])?, exclude: compile(&exc[server])?, ..Rules::default() };
        selected.insert(server.clone(), rules);
    }
    Ok(Profile { servers: selected, toon, timeout_secs })
}

/// Server prefix: the configured one, else the server's name.
pub fn prefix_of<'a>(name: &'a str, server: Option<&'a ServerConfig>) -> &'a str {
    server.and_then(|s| s.prefix.as_deref()).unwrap_or(name)
}
