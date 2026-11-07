//! ~/.config/rafa/gworkspace.yaml: `auth:` (secret refs) and optional `endpoints:`.
use std::collections::BTreeMap;

use gworkspace_auth::{auth_config, AuthConfig};
use rafa_config::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `None`: not configured (servers still start; every tool says how to configure).
    pub auth: Option<AuthConfig>,
    /// Service -> base URL, e.g. `tasks`, `token`. Defaults are Google's.
    pub endpoints: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Layer(#[from] rafa_config::ConfigError),
}

/// Typed view of the merged file (`None`: no file at all).
pub fn from_value(value: Option<&Value>) -> Result<Config, ConfigError> {
    let Some(value) = value else { return Ok(Config { auth: None, endpoints: BTreeMap::new() }) };
    let auth = match value.get("auth") {
        None | Some(Value::Null) => None,
        Some(a) => Some(auth_config(Some(a)).map_err(|e| ConfigError::Invalid(e.to_string()))?),
    };
    let mut endpoints = BTreeMap::new();
    if let Some(map) = value.get("endpoints").and_then(Value::as_mapping) {
        for (k, v) in map {
            let key = k.as_str().unwrap_or_default().to_string();
            let url = v.as_str().ok_or_else(|| ConfigError::Invalid(format!("endpoints.{key}: expected a URL string")))?;
            endpoints.insert(key, url.to_string());
        }
    }
    Ok(Config { auth, endpoints })
}

impl Config {
    /// The configured base URL for `service`, else `default`.
    pub fn endpoint(&self, service: &str, default: &str) -> String {
        self.endpoints.get(service).cloned().unwrap_or_else(|| default.to_string())
    }
}

/// Read and merge rafa/gworkspace.d/*.yaml and rafa/gworkspace.yaml, expand ${VAR}s.
pub fn load(env: &BTreeMap<String, String>) -> Result<Config, ConfigError> {
    let home = match env.get("XDG_CONFIG_HOME") {
        Some(x) if !x.is_empty() => x.clone(),
        _ => format!("{}/.config", env.get("HOME").map_or("", String::as_str)),
    };
    let dir = format!("{home}/rafa/gworkspace.d");
    let mut dropins: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| entries.filter_map(Result::ok).filter_map(|e| e.file_name().to_str().map(str::to_string)).filter(|n| n.ends_with(".yaml")).collect())
        .unwrap_or_default();
    dropins.sort();
    let mut layers = Vec::new();
    for path in rafa_config::layer_paths("gworkspace", &home, &dropins, None) {
        match std::fs::read_to_string(&path) {
            Ok(text) => layers.push(rafa_config::Layer::parse(&path, &text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ConfigError::Invalid(format!("{path}: {e}"))),
        }
    }
    if layers.is_empty() {
        return from_value(None);
    }
    let merged = rafa_config::merge(&layers, &[], 1)?;
    let value = rafa_config::expand(&merged.value, env, "gworkspace configuration")?;
    from_value(Some(&value))
}
