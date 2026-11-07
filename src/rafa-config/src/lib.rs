//! Layered configuration for first-party tools (ADR-0005).
//!
//! A tool reads `~/.config/rafa/<tool>.yaml`, layered over Nix-written drop-ins in
//! `~/.config/rafa/<tool>.d/*.yaml` and, only when asked for explicitly, a project's
//! `.rafa/<tool>.yaml`. Later layers win key by key. A named entry (for example a server
//! under `servers:`) defined by two layers is an error unless the later one says
//! `override: true`, so a drop-in and the user's file cannot silently fight.
//!
//! Strings expand `${VAR}` from a given environment; an unknown variable is an error.
//! Secrets are references, `{ file: PATH }` or `{ command: [ARGV...] }`, never values;
//! [`SecretRef::resolve`] is the only side effect and takes its runner as a parameter.

use std::collections::BTreeMap;

pub use serde_yaml_ng::Value;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("{origin}: invalid YAML: {reason}")]
    Parse { origin: String, reason: String },
    #[error("{origin}: unknown variable ${{{name}}} in {path}")]
    UnknownVariable { origin: String, path: String, name: String },
    #[error("{origin}: unterminated ${{...}} in {path}")]
    Unterminated { origin: String, path: String },
    #[error("{path} is defined in {first} and again in {second}; set `override: true` in {second} to replace it")]
    Duplicate { path: String, first: String, second: String },
    #[error("unsupported config version {found} in {origin} (expected {expected})")]
    Version { origin: String, found: i64, expected: i64 },
    #[error("{path}: not a secret reference (expected {{ file: PATH }} or {{ command: [ARGV...] }})")]
    NotASecret { path: String },
    #[error("secret {path}: {reason}")]
    Secret { path: String, reason: String },
}

/// One source of configuration, labelled for error messages and `--origin` output.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub origin: String,
    pub value: Value,
}

impl Layer {
    pub fn parse(origin: &str, text: &str) -> Result<Layer, ConfigError> {
        let value: Value = serde_yaml_ng::from_str(text)
            .map_err(|e| ConfigError::Parse { origin: origin.to_string(), reason: e.to_string() })?;
        let value = if value.is_null() { Value::Mapping(Default::default()) } else { value };
        Ok(Layer { origin: origin.to_string(), value })
    }
}

/// Expand `${VAR}` in every string of `value`.
pub fn expand(value: &Value, env: &BTreeMap<String, String>, origin: &str) -> Result<Value, ConfigError> {
    expand_at(value, env, origin, "")
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_string() } else { format!("{path}.{key}") }
}

fn key_text(key: &Value) -> String {
    match key {
        Value::String(s) => s.clone(),
        other => serde_yaml_ng::to_string(other).unwrap_or_default().trim().to_string(),
    }
}

fn expand_at(value: &Value, env: &BTreeMap<String, String>, origin: &str, path: &str) -> Result<Value, ConfigError> {
    Ok(match value {
        Value::String(s) => Value::String(expand_str(s, env, origin, path)?),
        Value::Sequence(items) => Value::Sequence(
            items.iter().enumerate().map(|(i, v)| expand_at(v, env, origin, &join(path, &i.to_string()))).collect::<Result<_, _>>()?,
        ),
        Value::Mapping(map) => {
            let mut out = serde_yaml_ng::Mapping::new();
            for (k, v) in map {
                out.insert(k.clone(), expand_at(v, env, origin, &join(path, &key_text(k)))?);
            }
            Value::Mapping(out)
        }
        other => other.clone(),
    })
}

fn expand_str(s: &str, env: &BTreeMap<String, String>, origin: &str, path: &str) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| ConfigError::Unterminated { origin: origin.to_string(), path: path.to_string() })?;
        let name = &after[..end];
        let value = env.get(name).ok_or_else(|| ConfigError::UnknownVariable {
            origin: origin.to_string(),
            path: path.to_string(),
            name: name.to_string(),
        })?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The merged configuration and, for every leaf path (`a.b.c`), the layer it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    pub value: Value,
    pub origins: BTreeMap<String, String>,
}

/// Merge layers in order. `named_maps` are the top-level keys whose entries are named
/// definitions (e.g. `servers`, `profiles`): an entry defined twice needs `override: true`.
/// Every layer's `version`, when present, must equal `version`.
pub fn merge(layers: &[Layer], named_maps: &[&str], version: i64) -> Result<Merged, ConfigError> {
    let mut merged = Merged { value: Value::Mapping(Default::default()), origins: BTreeMap::new() };
    // Which layer defined each named entry, e.g. "servers.slack" -> "10-nix.yaml".
    let mut owners: BTreeMap<String, String> = BTreeMap::new();
    for layer in layers {
        if let Some(found) = layer.value.get("version") {
            let found = found.as_i64().unwrap_or(-1);
            if found != version {
                return Err(ConfigError::Version { origin: layer.origin.clone(), found, expected: version });
            }
        }
        let Value::Mapping(top) = &layer.value else { continue };
        for (key, value) in top {
            let key_s = key_text(key);
            if named_maps.contains(&key_s.as_str()) {
                if let Value::Mapping(entries) = value {
                    for (name, entry) in entries {
                        let path = join(&key_s, &key_text(name));
                        let mut entry = entry.clone();
                        let overrides = match &mut entry {
                            Value::Mapping(m) => m.remove("override").and_then(|v| v.as_bool()).unwrap_or(false),
                            _ => false,
                        };
                        if let Some(first) = owners.get(&path) {
                            if !overrides {
                                return Err(ConfigError::Duplicate { path, first: first.clone(), second: layer.origin.clone() });
                            }
                            merged.origins.retain(|k, _| k != &path && !k.starts_with(&format!("{path}.")));
                        }
                        owners.insert(path.clone(), layer.origin.clone());
                        let slot = merged.value.as_mapping_mut().unwrap()
                            .entry(key.clone()).or_insert_with(|| Value::Mapping(Default::default()));
                        if !slot.is_mapping() { *slot = Value::Mapping(Default::default()); }
                        slot.as_mapping_mut().unwrap().insert(name.clone(), entry.clone());
                        record_origins(&entry, &path, &layer.origin, &mut merged.origins);
                    }
                    continue;
                }
            }
            let slot = merged.value.as_mapping_mut().unwrap();
            deep_merge(slot, key, value, &key_s, &layer.origin, &mut merged.origins);
        }
    }
    Ok(merged)
}

fn record_origins(value: &Value, path: &str, origin: &str, origins: &mut BTreeMap<String, String>) {
    match value {
        Value::Mapping(map) if !map.is_empty() => {
            for (k, v) in map {
                record_origins(v, &join(path, &key_text(k)), origin, origins);
            }
        }
        _ => {
            origins.insert(path.to_string(), origin.to_string());
        }
    }
}

fn deep_merge(
    into: &mut serde_yaml_ng::Mapping,
    key: &Value,
    value: &Value,
    path: &str,
    origin: &str,
    origins: &mut BTreeMap<String, String>,
) {
    match (into.get_mut(key), value) {
        (Some(Value::Mapping(existing)), Value::Mapping(incoming)) => {
            for (k, v) in incoming {
                deep_merge(existing, k, v, &join(path, &key_text(k)), origin, origins);
            }
        }
        _ => {
            origins.retain(|k, _| k != path && !k.starts_with(&format!("{path}.")));
            into.insert(key.clone(), value.clone());
            record_origins(value, path, origin, origins);
        }
    }
}

/// A secret reference as written in the config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretRef {
    File(String),
    Command(Vec<String>),
}

impl SecretRef {
    pub fn parse(value: &Value, path: &str) -> Result<SecretRef, ConfigError> {
        let not = || ConfigError::NotASecret { path: path.to_string() };
        let map = value.as_mapping().ok_or_else(not)?;
        if map.len() != 1 {
            return Err(not());
        }
        if let Some(file) = map.get("file").and_then(Value::as_str) {
            return Ok(SecretRef::File(file.to_string()));
        }
        let argv = map.get("command").and_then(Value::as_sequence).ok_or_else(not)?;
        let argv: Option<Vec<String>> = argv.iter().map(|a| a.as_str().map(str::to_string)).collect();
        match argv {
            Some(argv) if !argv.is_empty() => Ok(SecretRef::Command(argv)),
            _ => Err(not()),
        }
    }

    /// Read the secret: `read_file` for `File`, `run` for `Command` (argv, no shell).
    /// Surrounding whitespace is trimmed; an empty secret is an error.
    pub fn resolve(
        &self,
        path: &str,
        read_file: impl Fn(&str) -> Result<String, String>,
        run: impl Fn(&[String]) -> Result<String, String>,
    ) -> Result<String, ConfigError> {
        let raw = match self {
            SecretRef::File(file) => read_file(file),
            SecretRef::Command(argv) => run(argv),
        }
        .map_err(|reason| ConfigError::Secret { path: path.to_string(), reason })?;
        let secret = raw.trim();
        if secret.is_empty() {
            return Err(ConfigError::Secret { path: path.to_string(), reason: "the secret is empty".into() });
        }
        Ok(secret.to_string())
    }
}

/// Layer file paths for `tool`, in load order. `project` is used only when given explicitly.
pub fn layer_paths(tool: &str, config_home: &str, dropins: &[String], project: Option<&str>) -> Vec<String> {
    let mut sorted: Vec<&String> = dropins.iter().collect();
    sorted.sort();
    let mut paths: Vec<String> = sorted.into_iter().map(|d| format!("{config_home}/rafa/{tool}.d/{d}")).collect();
    paths.push(format!("{config_home}/rafa/{tool}.yaml"));
    if let Some(project) = project {
        paths.push(format!("{project}/.rafa/{tool}.yaml"));
    }
    paths
}
