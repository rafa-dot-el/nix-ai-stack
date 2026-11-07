//! The TUI's state and reducer. Pure: tool lists and layers in, edits of the user layer out.
use std::collections::BTreeMap;

use mcp_filter::{Catalog, Rules, Server};
use rafa_config::{merge, Layer, Value};
use rmcp::model::Tool;
use serde_yaml_ng::Mapping;

use crate::{
    config::{from_value, prefix_of, rules, McpConfig},
    proxy::filter_tool,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A drop-in (Home Manager): shown, never edited.
    Nix,
    /// The user's mcp.yaml: editable.
    User,
    /// Only discovered in the pool, no entry anywhere: editable (an entry is created).
    Pool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRow {
    pub original: String,
    /// `None`: hidden in the current scope.
    pub exposed: Option<String>,
    pub tokens: usize,
    pub read_only: bool,
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Server(String),
    /// (profile, server)
    Profile(String, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Servers,
    Profiles,
    Tools(Scope),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Back,
    Toggle,
    Tab,
    Save,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Write this value as the user's mcp.yaml.
    Save(Value),
    Quit,
}

/// One server's raw tools as the server lists them (`Err`: why it could not be listed).
pub type Inventory = BTreeMap<String, Result<Vec<Tool>, String>>;

#[derive(Debug, Clone)]
pub struct App {
    pub inventory: Inventory,
    pub dropins: Vec<Layer>,
    pub user: Layer,
    pub screen: Screen,
    pub cursor: usize,
    pub dirty: bool,
    pub message: Option<String>,
    /// A first quit with unsaved changes arms the second one.
    armed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerSummary {
    pub name: String,
    pub origin: Origin,
    pub shown: usize,
    pub total: usize,
    pub savings: mcp_tokens::Savings,
    pub error: Option<String>,
    /// Profiles that select this server.
    pub profiles: Vec<String>,
}

fn tokens(tool: &Tool) -> usize {
    mcp_tokens::tool_tokens(&tool.name, tool.description.as_deref(), &serde_json::Value::Object((*tool.input_schema).clone()))
}

fn s(text: &str) -> Value {
    Value::String(text.to_string())
}

fn defines(layer: &Layer, section: &str, name: &str) -> bool {
    layer.value.get(section).and_then(|m| m.get(name)).is_some()
}

/// The exposed name of `original` under a server's proxy-side rules (as mcp-filter names it).
fn proxy_name(prefix: &str, separator: &str, rules: &Rules, original: &str) -> String {
    let base = rules.rename.get(original).map_or(original, String::as_str);
    if prefix.is_empty() { base.to_string() } else { format!("{prefix}{separator}{base}") }
}

/// The YAML mapping at `path` in `doc`, created on the way.
fn entry_mut<'a>(doc: &'a mut Value, path: &[&str]) -> &'a mut Mapping {
    let mut cur = doc;
    for key in path {
        if !cur.is_mapping() {
            *cur = Value::Mapping(Mapping::new());
        }
        let map = cur.as_mapping_mut().expect("mapping");
        cur = map.entry(s(key)).or_insert_with(|| Value::Mapping(Mapping::new()));
    }
    if !cur.is_mapping() {
        *cur = Value::Mapping(Mapping::new());
    }
    cur.as_mapping_mut().expect("mapping")
}

fn list_mut<'a>(entry: &'a mut Mapping, key: &str) -> &'a mut Vec<Value> {
    let v = entry.entry(s(key)).or_insert_with(|| Value::Sequence(Vec::new()));
    if !v.is_sequence() {
        *v = Value::Sequence(Vec::new());
    }
    v.as_sequence_mut().expect("sequence")
}

/// Does the single pattern `item` (as written in YAML) match `tool`?
fn item_matches(item: &Value, tool: &mcp_filter::Tool) -> bool {
    let mut one = Mapping::new();
    one.insert(s("include"), Value::Sequence(vec![item.clone()]));
    rules(&Value::Mapping(one), "pattern").is_ok_and(|r| r.visible(tool))
}

fn describe(item: &Value) -> String {
    match item {
        Value::String(text) => text.clone(),
        other => serde_yaml_ng::to_string(other).unwrap_or_default().trim().replace('\n', " "),
    }
}

/// Flip one tool's visibility in a rules entry, keeping its mode (include- or exclude-based).
/// `Err` explains why it cannot be flipped from here.
fn flip(entry: &mut Mapping, tool: &mcp_filter::Tool, visible: bool) -> Result<(), String> {
    let name = s(&tool.name);
    let include: Vec<Value> = entry.get("include").and_then(Value::as_sequence).cloned().unwrap_or_default();
    let exclude: Vec<Value> = entry.get("exclude").and_then(Value::as_sequence).cloned().unwrap_or_default();
    if visible {
        let others_include = include.iter().any(|i| *i != name && item_matches(i, tool));
        if include.contains(&name) && !others_include {
            list_mut(entry, "include").retain(|i| *i != name);
            if entry.get("include").and_then(Value::as_sequence).is_some_and(Vec::is_empty) {
                // an empty include would mean "everything"; hide by exclusion instead
                entry.remove("include");
                list_mut(entry, "exclude").push(name);
            }
        } else {
            list_mut(entry, "exclude").push(name);
        }
        return Ok(());
    }
    if let Some(pattern) = exclude.iter().find(|e| **e != name && item_matches(e, tool)) {
        return Err(format!("{} is hidden by the exclude pattern {}; edit mcp.yaml to change it", tool.name, describe(pattern)));
    }
    if exclude.contains(&name) {
        list_mut(entry, "exclude").retain(|e| *e != name);
        if entry.get("exclude").and_then(Value::as_sequence).is_some_and(Vec::is_empty) {
            entry.remove("exclude");
        }
    }
    if !include.is_empty() && !include.iter().any(|i| item_matches(i, tool)) {
        list_mut(entry, "include").push(name);
    }
    Ok(())
}

impl App {
    pub fn new(inventory: Inventory, dropins: Vec<Layer>, user: Layer) -> App {
        App { inventory, dropins, user, screen: Screen::Servers, cursor: 0, dirty: false, message: None, armed: false }
    }

    fn layers(&self) -> Vec<Layer> {
        self.dropins.iter().cloned().chain([self.user.clone()]).collect()
    }

    fn config(&self) -> Result<McpConfig, String> {
        let merged = merge(&self.layers(), &["servers", "profiles"], 1).map_err(|e| e.to_string())?;
        from_value(&merged.value).map_err(|e| e.to_string())
    }

    fn origin(&self, section: &str, name: &str) -> Origin {
        if self.dropins.iter().any(|l| defines(l, section, name)) {
            Origin::Nix
        } else if defines(&self.user, section, name) {
            Origin::User
        } else {
            Origin::Pool
        }
    }

    fn server_names(&self, config: &McpConfig) -> Vec<String> {
        let mut names: Vec<String> = self.inventory.keys().chain(config.servers.keys()).cloned().collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn servers(&self) -> Vec<ServerSummary> {
        let config = self.config().unwrap_or_else(|_| from_value(&Value::Null).expect("empty config"));
        self.server_names(&config)
            .into_iter()
            .map(|name| {
                let empty = Rules::default();
                let rules = config.servers.get(&name).map_or(&empty, |s| &s.rules);
                let (tools, error) = match self.inventory.get(&name) {
                    Some(Ok(t)) => (t.as_slice(), None),
                    Some(Err(e)) => (&[][..], Some(e.clone())),
                    None => (&[][..], Some("not running".to_string())),
                };
                let kept: Vec<usize> = tools.iter().filter(|t| rules.visible(&filter_tool(t))).map(tokens).collect();
                let all: Vec<usize> = tools.iter().map(tokens).collect();
                let profiles = config.profiles.iter().filter(|(_, p)| p.servers.contains_key(&name)).map(|(n, _)| n.clone()).collect();
                ServerSummary {
                    origin: self.origin("servers", &name),
                    shown: kept.len(),
                    total: tools.len(),
                    savings: mcp_tokens::savings(&all, &kept),
                    error,
                    profiles,
                    name,
                }
            })
            .collect()
    }

    /// (profile name, origin)
    pub fn profiles(&self) -> Vec<(String, Origin)> {
        let config = self.config().unwrap_or_else(|_| from_value(&Value::Null).expect("empty config"));
        config.profiles.keys().map(|p| (p.clone(), self.origin("profiles", p))).collect()
    }

    /// One row per (profile, server) on the profiles screen.
    pub fn profile_rows(&self) -> Vec<(String, String, Origin)> {
        let config = self.config().unwrap_or_else(|_| from_value(&Value::Null).expect("empty config"));
        config
            .profiles
            .iter()
            .flat_map(|(p, profile)| profile.servers.keys().map(move |s| (p.clone(), s.clone())))
            .map(|(p, s)| {
                let origin = self.origin("profiles", &p);
                (p, s, origin)
            })
            .collect()
    }

    /// The rows of a tools screen.
    pub fn tools(&self, scope: &Scope) -> Vec<ToolRow> {
        let Ok(config) = self.config() else { return Vec::new() };
        let server = match scope {
            Scope::Server(s) | Scope::Profile(_, s) => s,
        };
        let Some(Ok(tools)) = self.inventory.get(server) else { return Vec::new() };
        let empty = Rules::default();
        let proxy_rules = config.servers.get(server).map_or(&empty, |s| &s.rules);
        let prefix = prefix_of(server, config.servers.get(server));
        let client = match scope {
            Scope::Server(_) => None,
            Scope::Profile(p, _) => Some(config.profiles.get(p).and_then(|p| p.servers.get(server)).unwrap_or(&empty)),
        };
        tools
            .iter()
            .map(|t| {
                let probe = filter_tool(t);
                let at_proxy = proxy_rules.visible(&probe);
                let exposed = match client {
                    None => at_proxy.then(|| proxy_name(prefix, &config.proxy.separator, proxy_rules, &t.name)),
                    Some(c) => (at_proxy && c.visible(&probe)).then(|| {
                        c.rename.get(t.name.as_ref()).cloned().unwrap_or_else(|| proxy_name(prefix, &config.proxy.separator, proxy_rules, &t.name))
                    }),
                };
                ToolRow {
                    original: t.name.to_string(),
                    exposed,
                    tokens: tokens(t),
                    read_only: probe.annotations.read_only == Some(true),
                    destructive: probe.annotations.destructive == Some(true),
                }
            })
            .collect()
    }

    fn rows(&self) -> usize {
        match &self.screen {
            Screen::Servers => self.servers().len(),
            Screen::Profiles => self.profile_rows().len(),
            Screen::Tools(scope) => self.tools(scope).len(),
        }
    }

    fn toggle(&mut self, scope: &Scope) -> Result<(), String> {
        let rows = self.tools(scope);
        let Some(row) = rows.get(self.cursor) else { return Ok(()) };
        let server = match scope {
            Scope::Server(s) | Scope::Profile(_, s) => s.clone(),
        };
        let tool = self.inventory[&server].as_ref().expect("listed").iter().find(|t| t.name == row.original).expect("row");
        let probe = filter_tool(tool);
        let path: Vec<&str> = match scope {
            Scope::Server(s) => {
                if self.origin("servers", s) == Origin::Nix {
                    return Err(format!("server {s} comes from Nix (a drop-in); change it there or in a profile of your own"));
                }
                vec!["servers", s.as_str()]
            }
            Scope::Profile(p, s) => {
                if self.origin("profiles", p) == Origin::Nix {
                    return Err(format!("profile {p} comes from Nix (a drop-in); change it there or make your own profile"));
                }
                let config = self.config()?;
                let empty = Rules::default();
                if !config.servers.get(s.as_str()).map_or(&empty, |x| &x.rules).visible(&probe) {
                    return Err(format!("{} is hidden by server {s}'s own rules; open the server to show it", row.original));
                }
                vec!["profiles", p.as_str(), "servers", s.as_str()]
            }
        };
        let mut value = self.user.value.clone();
        flip(entry_mut(&mut value, &path), &probe, row.exposed.is_some())?;
        self.user.value = value;
        self.dirty = true;
        Ok(())
    }

    /// The edited configuration must load and its full catalog must have no collisions.
    fn validate(&self) -> Result<(), String> {
        let config = self.config()?;
        let filter_tools: BTreeMap<&str, Vec<mcp_filter::Tool>> = self
            .inventory
            .iter()
            .filter_map(|(n, t)| t.as_ref().ok().map(|t| (n.as_str(), t.iter().map(filter_tool).collect())))
            .collect();
        let empty = Rules::default();
        let servers: Vec<Server<'_>> = filter_tools
            .iter()
            .map(|(name, tools)| Server {
                name,
                prefix: prefix_of(name, config.servers.get(*name)),
                rules: config.servers.get(*name).map_or(&empty, |s| &s.rules),
                tools,
            })
            .collect();
        Catalog::build(&servers, None, &config.proxy.separator).map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn update(&mut self, key: Key) -> Effect {
        if key != Key::Quit {
            self.armed = false;
        }
        self.message = None;
        match key {
            Key::Up => self.cursor = self.cursor.saturating_sub(1),
            Key::Down => self.cursor = (self.cursor + 1).min(self.rows().saturating_sub(1)),
            Key::Tab => {
                self.screen = match self.screen {
                    Screen::Servers => Screen::Profiles,
                    _ => Screen::Servers,
                };
                self.cursor = 0;
            }
            Key::Back => {
                self.screen = match &self.screen {
                    Screen::Tools(Scope::Profile(..)) => Screen::Profiles,
                    _ => Screen::Servers,
                };
                self.cursor = 0;
            }
            Key::Enter => match &self.screen {
                Screen::Servers => {
                    if let Some(server) = self.servers().into_iter().nth(self.cursor) {
                        match server.error {
                            Some(e) => self.message = Some(format!("{}: {e}", server.name)),
                            None => {
                                self.screen = Screen::Tools(Scope::Server(server.name));
                                self.cursor = 0;
                            }
                        }
                    }
                }
                Screen::Profiles => {
                    if let Some((p, s, _)) = self.profile_rows().into_iter().nth(self.cursor) {
                        self.screen = Screen::Tools(Scope::Profile(p, s));
                        self.cursor = 0;
                    }
                }
                Screen::Tools(_) => {}
            },
            Key::Toggle => {
                if let Screen::Tools(scope) = self.screen.clone() {
                    if let Err(e) = self.toggle(&scope) {
                        self.message = Some(e);
                    }
                }
            }
            Key::Save => {
                if !self.dirty {
                    self.message = Some("nothing to save".into());
                    return Effect::None;
                }
                if let Err(e) = self.validate() {
                    self.message = Some(format!("not saved: {e}"));
                    return Effect::None;
                }
                self.dirty = false;
                self.message = Some("saved".into());
                return Effect::Save(self.user.value.clone());
            }
            Key::Quit => {
                if self.dirty && !self.armed {
                    self.armed = true;
                    self.message = Some("unsaved changes: s saves, q again quits without saving".into());
                    return Effect::None;
                }
                return Effect::Quit;
            }
        }
        Effect::None
    }
}
