//! ~/.config/rafa/claude-usage.yaml (rafa.d drop-ins first): the credentials path, the API
//! endpoint, and the exporter's bind, port, path and cache TTL.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: Option<i64>,
    pub credentials_path: String,
    pub endpoint: String,
    pub exporter: Exporter,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Exporter {
    pub bind: String,
    pub port: u16,
    pub path: String,
    pub cache_ttl_secs: u64,
}

impl Default for Exporter {
    fn default() -> Self {
        Exporter { bind: "127.0.0.1".into(), port: 9464, path: "/metrics".into(), cache_ttl_secs: 60 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config { version: None, credentials_path: String::new(), endpoint: crate::client::API_URL.into(), exporter: Exporter::default() }
    }
}

/// The configuration (`file`: an explicit path instead of the default layers); the
/// credentials path defaults to ~/.claude/.credentials.json.
pub fn load(env: &BTreeMap<String, String>, file: Option<&str>) -> Result<Config, String> {
    let get = |k: &str| env.get(k).map(String::as_str).filter(|v| !v.is_empty());
    let home = get("HOME").unwrap_or("");
    let config_home = get("XDG_CONFIG_HOME").map_or_else(|| format!("{home}/.config"), str::to_string);
    let paths = match file {
        Some(f) => vec![f.to_string()],
        None => {
            let mut dropins: Vec<String> = std::fs::read_dir(format!("{config_home}/rafa/claude-usage.d"))
                .map(|es| es.filter_map(Result::ok).filter_map(|e| e.file_name().to_str().map(str::to_string)).filter(|n| n.ends_with(".yaml")).collect())
                .unwrap_or_default();
            dropins.sort();
            rafa_config::layer_paths("claude-usage", &config_home, &dropins, None)
        }
    };
    let mut layers = Vec::new();
    for path in paths {
        match std::fs::read_to_string(&path) {
            Ok(text) => layers.push(rafa_config::Layer::parse(&path, &text).map_err(|e| e.to_string())?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && file.is_none() => {}
            Err(e) => return Err(format!("{path}: {e}")),
        }
    }
    let mut cfg = if layers.is_empty() {
        Config::default()
    } else {
        let merged = rafa_config::merge(&layers, &[], 1).map_err(|e| e.to_string())?;
        let value = rafa_config::expand(&merged.value, env, "claude-usage configuration").map_err(|e| e.to_string())?;
        serde_yaml_ng::from_value(value).map_err(|e| format!("claude-usage.yaml: {e}"))?
    };
    if cfg.credentials_path.is_empty() {
        cfg.credentials_path = format!("{home}/.claude/.credentials.json");
    }
    Ok(cfg)
}
