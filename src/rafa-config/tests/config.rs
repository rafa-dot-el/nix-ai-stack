use std::collections::BTreeMap;

use rafa_config::{expand, layer_paths, merge, ConfigError, Layer, SecretRef, Value};

fn layer(origin: &str, text: &str) -> Layer {
    Layer::parse(origin, text).unwrap()
}

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn get<'a>(v: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(v, |v, k| &v[k])
}

// -- parse / expand ---------------------------------------------------------------------

#[test]
fn parse_reports_the_origin_of_bad_yaml() {
    let err = Layer::parse("user.yaml", "a: [1, 2").unwrap_err();
    assert!(matches!(err, ConfigError::Parse { ref origin, .. } if origin == "user.yaml"));
}

#[test]
fn empty_file_is_an_empty_mapping() {
    let l = layer("x", "");
    assert!(merge(&[l], &[], 1).unwrap().value.as_mapping().unwrap().is_empty());
}

#[test]
fn expand_replaces_variables_in_nested_strings_only() {
    let l = layer("x", "socket: ${XDG_RUNTIME_DIR}/rafa/mcp.sock\nlist: [\"${HOME}/a\", 3]\nn: 5\n");
    let out = expand(&l.value, &env(&[("XDG_RUNTIME_DIR", "/run/user/1"), ("HOME", "/h")]), "x").unwrap();
    assert_eq!(get(&out, "socket").as_str(), Some("/run/user/1/rafa/mcp.sock"));
    assert_eq!(out["list"][0].as_str(), Some("/h/a"));
    assert_eq!(out["list"][1].as_i64(), Some(3));
    assert_eq!(get(&out, "n").as_i64(), Some(5));
}

#[test]
fn expand_fails_on_unknown_and_unterminated_variables_with_their_path() {
    let l = layer("user.yaml", "a:\n  b: ${NOPE}/x\n");
    assert_eq!(
        expand(&l.value, &env(&[]), "user.yaml").unwrap_err(),
        ConfigError::UnknownVariable { origin: "user.yaml".into(), path: "a.b".into(), name: "NOPE".into() }
    );
    let l = layer("user.yaml", "a: ${HOME\n");
    assert!(matches!(expand(&l.value, &env(&[("HOME", "/h")]), "user.yaml"), Err(ConfigError::Unterminated { .. })));
}

#[test]
fn a_lone_dollar_is_left_alone() {
    let l = layer("x", "price: costs $5\n");
    assert_eq!(expand(&l.value, &env(&[]), "x").unwrap()["price"].as_str(), Some("costs $5"));
}

// -- merge ------------------------------------------------------------------------------

#[test]
fn later_layers_win_key_by_key_and_origins_are_tracked() {
    let base = layer("10-nix.yaml", "proxy:\n  socket: /a\n  log: { level: info }\n");
    let user = layer("mcp.yaml", "proxy:\n  log: { level: debug }\n");
    let merged = merge(&[base, user], &["servers"], 1).unwrap();
    assert_eq!(get(&merged.value, "proxy.socket").as_str(), Some("/a"));
    assert_eq!(get(&merged.value, "proxy.log.level").as_str(), Some("debug"));
    assert_eq!(merged.origins["proxy.socket"], "10-nix.yaml");
    assert_eq!(merged.origins["proxy.log.level"], "mcp.yaml");
}

#[test]
fn lists_are_replaced_not_concatenated() {
    let a = layer("a", "include: [x, y]\n");
    let b = layer("b", "include: [z]\n");
    let merged = merge(&[a, b], &[], 1).unwrap();
    assert_eq!(merged.value["include"].as_sequence().unwrap().len(), 1);
    assert_eq!(merged.origins["include"], "b");
}

#[test]
fn a_named_entry_defined_twice_needs_override() {
    let nix = layer("10-nix.yaml", "servers:\n  slack: { prefix: slack }\n");
    let user = layer("mcp.yaml", "servers:\n  slack: { prefix: chat }\n");
    assert_eq!(
        merge(&[nix.clone(), user], &["servers"], 1).unwrap_err(),
        ConfigError::Duplicate { path: "servers.slack".into(), first: "10-nix.yaml".into(), second: "mcp.yaml".into() }
    );
    let user = layer("mcp.yaml", "servers:\n  slack: { override: true, prefix: chat }\n");
    let merged = merge(&[nix, user], &["servers"], 1).unwrap();
    assert_eq!(get(&merged.value, "servers.slack.prefix").as_str(), Some("chat"));
    // the marker itself does not leak into the result
    assert!(merged.value["servers"]["slack"].get("override").is_none());
}

#[test]
fn override_replaces_the_whole_entry() {
    let nix = layer("a", "servers:\n  s: { prefix: p, timeout: 30 }\n");
    let user = layer("b", "servers:\n  s: { override: true, prefix: q }\n");
    let merged = merge(&[nix, user], &["servers"], 1).unwrap();
    assert!(merged.value["servers"]["s"].get("timeout").is_none());
}

#[test]
fn different_named_entries_from_different_layers_coexist() {
    let nix = layer("a", "servers:\n  slack: { prefix: slack }\n");
    let user = layer("b", "servers:\n  scratch: { prefix: s }\n");
    let merged = merge(&[nix, user], &["servers"], 1).unwrap();
    assert_eq!(merged.value["servers"].as_mapping().unwrap().len(), 2);
}

#[test]
fn version_must_match_when_present() {
    let ok = layer("a", "version: 1\n");
    let none = layer("b", "x: 1\n");
    assert!(merge(&[ok, none], &[], 1).is_ok());
    let bad = layer("c", "version: 2\n");
    assert_eq!(merge(&[bad], &[], 1).unwrap_err(), ConfigError::Version { origin: "c".into(), found: 2, expected: 1 });
}

// -- secrets ----------------------------------------------------------------------------

#[test]
fn secret_refs_parse_file_and_command_and_reject_inline_values() {
    let l = layer("x", "a: { file: /run/c/token }\nb: { command: [pass, show, slack/token] }\nc: hunter2\n");
    assert_eq!(SecretRef::parse(&l.value["a"], "a").unwrap(), SecretRef::File("/run/c/token".into()));
    assert_eq!(
        SecretRef::parse(&l.value["b"], "b").unwrap(),
        SecretRef::Command(vec!["pass".into(), "show".into(), "slack/token".into()])
    );
    assert_eq!(SecretRef::parse(&l.value["c"], "c").unwrap_err(), ConfigError::NotASecret { path: "c".into() });
    let empty = layer("x", "d: { command: [] }\n");
    assert!(matches!(SecretRef::parse(&empty.value["d"], "d"), Err(ConfigError::NotASecret { .. })));
}

#[test]
fn resolve_uses_the_right_runner_and_trims() {
    let file = SecretRef::File("/t".into());
    let cmd = SecretRef::Command(vec!["pass".into(), "show".into(), "x".into()]);
    let read = |p: &str| if p == "/t" { Ok("  from-file\n".to_string()) } else { Err("no".to_string()) };
    let run = |argv: &[String]| Ok(format!("{}\n", argv.join(" ")));
    assert_eq!(file.resolve("a", read, run).unwrap(), "from-file");
    assert_eq!(cmd.resolve("b", read, run).unwrap(), "pass show x");
}

#[test]
fn resolve_fails_on_runner_errors_and_empty_secrets() {
    let cmd = SecretRef::Command(vec!["false".into()]);
    let err = cmd.resolve("tok", |_| Ok(String::new()), |_| Err("exit 1".into())).unwrap_err();
    assert_eq!(err, ConfigError::Secret { path: "tok".into(), reason: "exit 1".into() });
    let file = SecretRef::File("/t".into());
    let err = file.resolve("tok", |_| Ok("  \n".into()), |_| Ok(String::new())).unwrap_err();
    assert!(matches!(err, ConfigError::Secret { ref reason, .. } if reason.contains("empty")));
}

// -- layer paths ------------------------------------------------------------------------

#[test]
fn layer_paths_are_dropins_sorted_then_user_then_explicit_project() {
    let dropins = vec!["20-b.yaml".to_string(), "10-a.yaml".to_string()];
    assert_eq!(
        layer_paths("mcp", "/h/.config", &dropins, None),
        vec!["/h/.config/rafa/mcp.d/10-a.yaml", "/h/.config/rafa/mcp.d/20-b.yaml", "/h/.config/rafa/mcp.yaml"]
    );
    assert_eq!(
        layer_paths("mcp", "/h/.config", &[], Some("/p")),
        vec!["/h/.config/rafa/mcp.yaml", "/p/.rafa/mcp.yaml"]
    );
}
