//! `mcp doctor`: what the proxy serves, what each profile exposes, what it saves, what is wrong.
//! Pure: built from the loaded config and the proxy's tool list.
use std::collections::BTreeMap;

use mcp_tokens::{savings, tool_tokens, Savings};
use rmcp::model::Tool;
use serde_json::{json, Value};

use crate::{
    client::view,
    config::McpConfig,
    proxy::{META_ORIGINAL, META_SERVER},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ServerReport {
    pub name: String,
    pub tools: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileReport {
    pub name: String,
    pub exposed: Vec<String>,
    pub savings: Savings,
    /// (server, rule) pairs naming tools the server no longer offers.
    pub stale: Vec<(String, String)>,
    /// Servers the profile names that the proxy does not serve.
    pub missing: Vec<String>,
    /// A view error (e.g. a rename collision).
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub proxy: Result<Vec<ServerReport>, String>,
    pub profiles: Vec<ProfileReport>,
}

impl Report {
    /// No findings: the proxy answers and every profile is clean.
    pub fn ok(&self) -> bool {
        self.proxy.is_ok() && self.profiles.iter().all(|p| p.stale.is_empty() && p.missing.is_empty() && p.error.is_none())
    }
}

fn meta<'a>(tool: &'a Tool, key: &str) -> Option<&'a str> {
    tool.meta.as_ref()?.0.get(key)?.as_str()
}

fn tokens(tool: &Tool) -> usize {
    tool_tokens(&tool.name, tool.description.as_deref(), &Value::Object((*tool.input_schema).clone()))
}

/// `only`: report one profile (an unknown name is an error); `None`: all of them.
pub fn report(config: &McpConfig, proxy_tools: Result<&[Tool], String>, only: Option<&str>) -> Result<Report, String> {
    if let Some(name) = only {
        if !config.profiles.contains_key(name) {
            let known: Vec<&str> = config.profiles.keys().map(String::as_str).collect();
            return Err(format!("no profile {name:?} (known: {})", known.join(", ")));
        }
    }
    let tools = match proxy_tools {
        Ok(tools) => tools,
        Err(e) => return Ok(Report { proxy: Err(e), profiles: Vec::new() }),
    };
    let mut by_server: BTreeMap<&str, Vec<&Tool>> = BTreeMap::new();
    for tool in tools {
        if let Some(server) = meta(tool, META_SERVER) {
            by_server.entry(server).or_default().push(tool);
        }
    }
    let servers = by_server.iter().map(|(name, t)| ServerReport { name: name.to_string(), tools: t.len() }).collect();
    let mut profiles = Vec::new();
    for (name, profile) in config.profiles.iter().filter(|(n, _)| only.is_none_or(|o| o == n.as_str())) {
        let (mut stale, mut missing, mut all) = (Vec::new(), Vec::new(), Vec::new());
        for (server, rules) in &profile.servers {
            let Some(offered) = by_server.get(server.as_str()) else {
                missing.push(server.clone());
                continue;
            };
            all.extend(offered.iter().map(|t| tokens(t)));
            let originals: Vec<mcp_filter::Tool> =
                offered.iter().filter_map(|t| meta(t, META_ORIGINAL)).map(mcp_filter::Tool::named).collect();
            stale.extend(mcp_filter::stale(rules, &originals).into_iter().map(|rule| (server.clone(), rule)));
        }
        let (exposed, kept, error) = match view(tools, profile) {
            Ok(v) => (v.tools.iter().map(|t| t.name.to_string()).collect(), v.tools.iter().map(tokens).collect(), None),
            Err(e) => (Vec::new(), Vec::new(), Some(e.to_string())),
        };
        profiles.push(ProfileReport { name: name.clone(), exposed, savings: savings(&all, &kept), stale, missing, error });
    }
    Ok(Report { proxy: Ok(servers), profiles })
}

pub fn render_text(report: &Report) -> String {
    let mut out = Vec::new();
    match &report.proxy {
        Err(e) => out.push(format!("[-] proxy: {e}")),
        Ok(servers) => {
            let total: usize = servers.iter().map(|s| s.tools).sum();
            out.push(format!("[+] proxy: {} servers, {total} tools", servers.len()));
            out.extend(servers.iter().map(|s| format!("[*]   {}: {} tools", s.name, s.tools)));
        }
    }
    for p in &report.profiles {
        let clean = p.stale.is_empty() && p.missing.is_empty() && p.error.is_none();
        let s = &p.savings;
        out.push(format!(
            "{} profile {}: {} tools, {} -> {} tokens ({:.1}% saved, estimate)",
            if clean { "[+]" } else { "[-]" },
            p.name,
            p.exposed.len(),
            s.full,
            s.filtered,
            s.percent_saved
        ));
        out.extend(p.stale.iter().map(|(server, rule)| format!("[-]   stale rule {server}:{rule}")));
        out.extend(p.missing.iter().map(|server| format!("[-]   missing server {server}")));
        out.extend(p.error.iter().map(|e| format!("[-]   {e}")));
    }
    out.join("\n") + "\n"
}

pub fn render_json(report: &Report) -> Value {
    let (servers, proxy_error) = match &report.proxy {
        Ok(s) => (s.iter().map(|s| json!({ "name": s.name, "tools": s.tools })).collect(), Value::Null),
        Err(e) => (Vec::new(), Value::String(e.clone())),
    };
    let profiles: Vec<Value> = report
        .profiles
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "exposed": p.exposed,
                "tokens": { "full": p.savings.full, "filtered": p.savings.filtered, "percent_saved": p.savings.percent_saved },
                "stale": p.stale.iter().map(|(s, r)| json!({ "server": s, "rule": r })).collect::<Vec<_>>(),
                "missing": p.missing,
                "error": p.error,
            })
        })
        .collect();
    json!({ "ok": report.ok(), "proxy_error": proxy_error, "servers": servers, "profiles": profiles })
}
