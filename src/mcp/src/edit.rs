//! `mcp config add-mcp|add-profile|remove-*|show`: pure edits of the user's own layer
//! (`~/.config/rafa/mcp.yaml`). Every edit is validated against all layers before it is
//! returned; drop-ins (Nix) are never edited. Comments in the user file are not preserved.
use rafa_config::{merge, Layer, Value};
use serde_yaml_ng::Mapping;

use crate::config::{from_value, qualify, Transport};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EditError {
    #[error("{kind} {name:?} already exists in {origin}")]
    Exists { kind: &'static str, name: String, origin: String },
    #[error("{kind} {name:?} is defined in {origin}, not in your mcp.yaml; change it there")]
    NotYours { kind: &'static str, name: String, origin: String },
    #[error("no {kind} named {name:?}")]
    Missing { kind: &'static str, name: String },
    #[error("the result would be invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Server,
    Profile,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerSpec {
    /// `None`: rules for a server discovered in the pool.
    pub transport: Option<Transport>,
    pub prefix: Option<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// `tool=TEXT` pairs.
    pub describe: Vec<(String, String)>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileSpec {
    pub servers: Vec<String>,
    /// Unqualified patterns apply to every server, `server:pattern` to one (as `mcp client`).
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub toon: bool,
    pub timeout_secs: Option<u64>,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Server => "servers",
            Kind::Profile => "profiles",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Kind::Server => "server",
            Kind::Profile => "profile",
        }
    }
}

const NAMED: [&str; 2] = ["servers", "profiles"];

fn s(text: &str) -> Value {
    Value::String(text.to_string())
}

fn list(items: &[String]) -> Value {
    Value::Sequence(items.iter().map(|i| s(i)).collect())
}

/// The first layer that defines `kind.name`.
fn origin_of<'a>(layers: impl IntoIterator<Item = &'a Layer>, kind: Kind, name: &str) -> Option<String> {
    layers.into_iter().find(|l| l.value.get(kind.key()).and_then(|m| m.get(name)).is_some()).map(|l| l.origin.clone())
}

/// The user layer with `kind.name = entry`, validated against every layer.
fn with_entry(user: &Layer, others: &[Layer], kind: Kind, name: &str, entry: Value) -> Result<Value, EditError> {
    if let Some(origin) = origin_of(others.iter().chain([user]), kind, name) {
        return Err(EditError::Exists { kind: kind.label(), name: name.to_string(), origin });
    }
    let mut doc = match &user.value {
        Value::Mapping(m) => m.clone(),
        _ => Mapping::new(),
    };
    let section = doc.entry(s(kind.key())).or_insert_with(|| Value::Mapping(Mapping::new()));
    if !section.is_mapping() {
        *section = Value::Mapping(Mapping::new());
    }
    section.as_mapping_mut().expect("mapping").insert(s(name), entry);
    let value = Value::Mapping(doc);
    validate(others, &user.origin, &value)?;
    Ok(value)
}

fn validate(others: &[Layer], origin: &str, value: &Value) -> Result<(), EditError> {
    let mut layers = others.to_vec();
    layers.push(Layer { origin: origin.to_string(), value: value.clone() });
    let merged = merge(&layers, &NAMED, 1).map_err(|e| EditError::Invalid(e.to_string()))?;
    from_value(&merged.value).map(|_| ()).map_err(|e| EditError::Invalid(e.to_string()))
}

fn rules_entry(entry: &mut Mapping, include: &[String], exclude: &[String]) {
    if !include.is_empty() {
        entry.insert(s("include"), list(include));
    }
    if !exclude.is_empty() {
        entry.insert(s("exclude"), list(exclude));
    }
}

/// Add a server entry to `user`; `others` are the drop-in layers (read-only).
pub fn add_server(user: &Layer, others: &[Layer], name: &str, spec: &ServerSpec) -> Result<Value, EditError> {
    let mut entry = Mapping::new();
    match &spec.transport {
        Some(Transport::Unix(path)) => {
            entry.insert(s("transport"), Value::Mapping(Mapping::from_iter([(s("unix"), s(path))])));
        }
        Some(Transport::Stdio { command, args, env }) => {
            let mut stdio = Mapping::from_iter([(s("command"), s(command))]);
            if !args.is_empty() {
                stdio.insert(s("args"), list(args));
            }
            if !env.is_empty() {
                stdio.insert(s("env"), Value::Mapping(env.iter().map(|(k, v)| (s(k), s(v))).collect()));
            }
            entry.insert(s("transport"), Value::Mapping(Mapping::from_iter([(s("stdio"), Value::Mapping(stdio))])));
        }
        None => {}
    }
    if let Some(prefix) = &spec.prefix {
        entry.insert(s("prefix"), s(prefix));
    }
    rules_entry(&mut entry, &spec.include, &spec.exclude);
    if !spec.describe.is_empty() {
        entry.insert(s("describe"), Value::Mapping(spec.describe.iter().map(|(k, v)| (s(k), s(v))).collect()));
    }
    if let Some(secs) = spec.timeout_secs {
        entry.insert(s("timeout"), Value::Number(secs.into()));
    }
    with_entry(user, others, Kind::Server, name, Value::Mapping(entry))
}

pub fn add_profile(user: &Layer, others: &[Layer], name: &str, spec: &ProfileSpec) -> Result<Value, EditError> {
    if let Some(origin) = origin_of(others.iter().chain([user]), Kind::Profile, name) {
        return Err(EditError::Exists { kind: "profile", name: name.to_string(), origin });
    }
    if spec.servers.is_empty() {
        return Err(EditError::Invalid(format!("profile {name:?} needs at least one --server")));
    }
    let invalid = |e: crate::config::McpConfigError| EditError::Invalid(e.to_string());
    let include = qualify(&spec.servers, &spec.include, "--include").map_err(invalid)?;
    let exclude = qualify(&spec.servers, &spec.exclude, "--exclude").map_err(invalid)?;
    let mut servers = Mapping::new();
    for server in &spec.servers {
        let mut rules = Mapping::new();
        rules_entry(&mut rules, &include[server], &exclude[server]);
        servers.insert(s(server), Value::Mapping(rules));
    }
    let mut entry = Mapping::from_iter([(s("servers"), Value::Mapping(servers))]);
    if spec.toon {
        entry.insert(s("toon"), Value::Bool(true));
    }
    if let Some(secs) = spec.timeout_secs {
        entry.insert(s("timeout"), Value::Number(secs.into()));
    }
    with_entry(user, others, Kind::Profile, name, Value::Mapping(entry))
}

pub fn remove(user: &Layer, others: &[Layer], kind: Kind, name: &str) -> Result<Value, EditError> {
    let mut value = user.value.clone();
    let removed = value.get_mut(kind.key()).and_then(Value::as_mapping_mut).and_then(|m| m.remove(name));
    if removed.is_none() {
        return Err(match origin_of(others, kind, name) {
            Some(origin) => EditError::NotYours { kind: kind.label(), name: name.to_string(), origin },
            None => EditError::Missing { kind: kind.label(), name: name.to_string() },
        });
    }
    validate(others, &user.origin, &value)?;
    Ok(value)
}

/// The merged configuration as YAML; with `origins`, one `path<TAB>file` line per leaf instead.
pub fn show(layers: &[Layer], origins: bool) -> Result<String, EditError> {
    let merged = merge(layers, &NAMED, 1).map_err(|e| EditError::Invalid(e.to_string()))?;
    if origins {
        return Ok(merged.origins.iter().map(|(path, origin)| format!("{path}\t{origin}\n")).collect());
    }
    serde_yaml_ng::to_string(&merged.value).map_err(|e| EditError::Invalid(e.to_string()))
}
