//! Where things are: ~/.config/rafa/tmpmail.yaml (rafa.d drop-ins first; `endpoint`
//! overrides the API) and the identities file under $XDG_STATE_HOME/rafa/tmpmail/.
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub endpoint: String,
    pub identities: PathBuf,
}

fn non_empty<'a>(env: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    env.get(key).map(String::as_str).filter(|v| !v.is_empty())
}

pub fn load(env: &BTreeMap<String, String>) -> Result<Setup, String> {
    let home = non_empty(env, "HOME").unwrap_or("");
    let config = non_empty(env, "XDG_CONFIG_HOME").map_or_else(|| format!("{home}/.config"), str::to_string);
    let state = non_empty(env, "XDG_STATE_HOME").map_or_else(|| format!("{home}/.local/state"), str::to_string);
    let mut dropins: Vec<String> = std::fs::read_dir(format!("{config}/rafa/tmpmail.d"))
        .map(|es| es.filter_map(Result::ok).filter_map(|e| e.file_name().to_str().map(str::to_string)).filter(|n| n.ends_with(".yaml")).collect())
        .unwrap_or_default();
    dropins.sort();
    let mut layers = Vec::new();
    for path in rafa_config::layer_paths("tmpmail", &config, &dropins, None) {
        match std::fs::read_to_string(&path) {
            Ok(text) => layers.push(rafa_config::Layer::parse(&path, &text).map_err(|e| e.to_string())?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{path}: {e}")),
        }
    }
    let mut endpoint = crate::api::BASE.to_string();
    if !layers.is_empty() {
        let merged = rafa_config::merge(&layers, &[], 1).map_err(|e| e.to_string())?;
        let value = rafa_config::expand(&merged.value, env, "tmpmail configuration").map_err(|e| e.to_string())?;
        if let Some(map) = value.as_mapping() {
            for (k, v) in map {
                match k.as_str() {
                    Some("version") => {}
                    Some("endpoint") => endpoint = v.as_str().ok_or("tmpmail.yaml: endpoint: expected a URL")?.to_string(),
                    other => return Err(format!("tmpmail.yaml: unknown key {other:?} (version, endpoint)")),
                }
            }
        }
    }
    Ok(Setup { endpoint, identities: PathBuf::from(format!("{state}/rafa/tmpmail/identities.yaml")) })
}
