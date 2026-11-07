use std::sync::Arc;

use mcp::{
    config::from_value,
    doctor::{render_json, render_text, report},
    proxy::{META_ORIGINAL, META_SERVER},
};
use rmcp::model::Tool;

fn proxied(server: &str, original: &str) -> Tool {
    let mut t = Tool::new(format!("{server}-{original}"), format!("{original}: does a thing with some words"), Arc::new(serde_json::Map::new()));
    let mut meta = rmcp::model::MetaObject::default();
    meta.0.insert(META_SERVER.into(), server.into());
    meta.0.insert(META_ORIGINAL.into(), original.into());
    t.meta = Some(meta);
    t
}

fn tools() -> Vec<Tool> {
    vec![proxied("slack", "search"), proxied("slack", "post"), proxied("slack", "history"), proxied("gtasks", "list_tasks")]
}

const CONFIG: &str = "
profiles:
  comms: { servers: { slack: { include: [search, history] } } }
  broken: { servers: { slack: { include: [gone_tool], rename: { search: gtasks-list_tasks } }, web: {} } }
";

fn config() -> mcp::config::McpConfig {
    from_value(&rafa_config::Layer::parse("t", CONFIG).unwrap().value).unwrap()
}

#[test]
fn servers_and_profile_savings_are_reported() {
    let t = tools();
    let r = report(&config(), Ok(&t), Some("comms")).unwrap();
    let servers = r.proxy.as_ref().unwrap();
    assert_eq!(servers.iter().map(|s| (s.name.as_str(), s.tools)).collect::<Vec<_>>(), vec![("gtasks", 1), ("slack", 3)]);
    let p = &r.profiles[0];
    assert_eq!(p.exposed, vec!["slack-history", "slack-search"]);
    assert!(p.savings.full > p.savings.filtered && p.savings.filtered > 0);
    assert!((p.savings.percent_saved - 100.0 * (p.savings.full - p.savings.filtered) as f64 / p.savings.full as f64).abs() < 1e-9);
    assert!(r.ok());
}

#[test]
fn stale_rules_missing_servers_and_view_errors_are_findings() {
    let t = tools();
    let r = report(&config(), Ok(&t), Some("broken")).unwrap();
    let p = &r.profiles[0];
    assert_eq!(p.stale, vec![("slack".to_string(), "gone_tool".to_string())]);
    assert_eq!(p.missing, vec!["web".to_string()]);
    assert!(!r.ok());
    let all = report(&config(), Ok(&t), None).unwrap();
    assert_eq!(all.profiles.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["broken", "comms"]);
}

#[test]
fn a_view_collision_is_reported_not_fatal() {
    let yaml = "profiles:\n  c: { servers: { slack: { rename: { post: slack-search } } } }\n";
    let c = from_value(&rafa_config::Layer::parse("t", yaml).unwrap().value).unwrap();
    let t = tools();
    let r = report(&c, Ok(&t), None).unwrap();
    assert!(r.profiles[0].error.as_deref().unwrap().contains("slack-search"));
    assert!(!r.ok());
}

#[test]
fn a_dead_proxy_is_a_finding() {
    let r = report(&config(), Err("connect: refused".into()), None).unwrap();
    assert!(r.proxy.is_err() && r.profiles.is_empty() && !r.ok());
    assert!(render_text(&r).contains("[-] proxy: connect: refused"));
}

#[test]
fn an_unknown_profile_is_an_error() {
    let t = tools();
    assert!(report(&config(), Ok(&t), Some("nope")).unwrap_err().contains("nope"));
}

#[test]
fn renderings_carry_the_findings() {
    let t = tools();
    let r = report(&config(), Ok(&t), None).unwrap();
    let text = render_text(&r);
    assert!(text.contains("[+] proxy: 2 servers, 4 tools"), "{text}");
    assert!(text.contains("[-] profile broken"), "{text}");
    assert!(text.contains("stale rule slack:gone_tool"), "{text}");
    assert!(text.contains("[+] profile comms: 2 tools"), "{text}");
    assert!(text.contains("% saved"), "{text}");
    let json = render_json(&r);
    assert_eq!(json["ok"], false);
    assert_eq!(json["servers"][1]["name"], "slack");
    assert_eq!(json["profiles"][1]["exposed"][0], "slack-history");
    assert_eq!(json["profiles"][0]["missing"][0], "web");
}
