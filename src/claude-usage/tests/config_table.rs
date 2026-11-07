use std::collections::{BTreeMap, HashMap};

use claude_usage::{config::load, table::table};

fn home(tag: &str, yaml: Option<&str>) -> (std::path::PathBuf, BTreeMap<String, String>) {
    let d = std::env::temp_dir().join(format!("cu-cfg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/rafa")).unwrap();
    if let Some(y) = yaml {
        std::fs::write(d.join("cfg/rafa/claude-usage.yaml"), y).unwrap();
    }
    let env = BTreeMap::from([("HOME".to_string(), d.display().to_string()), ("XDG_CONFIG_HOME".to_string(), d.join("cfg").display().to_string())]);
    (d, env)
}

#[test]
fn defaults_layers_and_errors() {
    let (d, env) = home("def", None);
    let c = load(&env, None).unwrap();
    assert_eq!(c.credentials_path, d.join(".claude/.credentials.json").display().to_string());
    assert_eq!((c.exporter.bind.as_str(), c.exporter.port, c.exporter.path.as_str()), ("127.0.0.1", 9464, "/metrics"));
    assert_eq!(c.endpoint, claude_usage::client::API_URL);
    let (_, env) = home("set", Some("version: 1\ncredentials_path: ${HOME}/c.json\nexporter: { port: 9999 }\n"));
    let c = load(&env, None).unwrap();
    assert!(c.credentials_path.ends_with("/c.json") && !c.credentials_path.contains('$'));
    assert_eq!((c.exporter.port, c.exporter.cache_ttl_secs), (9999, 60));
    let (d, env) = home("bad", Some("exporter: { prot: 1 }\n"));
    assert!(load(&env, None).unwrap_err().contains("prot"));
    std::fs::write(d.join("x.yaml"), "endpoint: http://127.0.0.1:1/u\n").unwrap();
    assert_eq!(load(&env, Some(d.join("x.yaml").to_str().unwrap())).unwrap().endpoint, "http://127.0.0.1:1/u", "an explicit file replaces the layers");
    assert!(load(&env, Some("/nonexistent/x.yaml")).is_err());
}

#[test]
fn the_table_is_plain_or_coloured() {
    let u: claude_usage::model::UsageData = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/usage_multi_model.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
    let vars: HashMap<String, String> = claude_usage::vars::build(&u);
    let plain = table(&u, &vars, false);
    assert!(plain.starts_with("Claude Code Usage\n") && plain.contains("5-hour session") && plain.contains("7-day overall"), "{plain}");
    assert!(!plain.contains('\x1b'));
    let coloured = table(&u, &vars, true);
    assert!(coloured.contains("\x1b[1m") && coloured.contains("\x1b[0m"));
    assert!(plain.contains("Weekly ("), "per-model rows: {plain}");
}
