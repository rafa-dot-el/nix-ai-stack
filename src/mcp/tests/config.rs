use mcp::config::{from_value, inline_profile, prefix_of, McpConfigError, Transport, DEFAULT_SEPARATOR};
use mcp_filter::Tool;
use rafa_config::Layer;

fn cfg(text: &str) -> mcp::config::McpConfig {
    from_value(&Layer::parse("t", text).unwrap().value).unwrap()
}

#[test]
fn empty_config_gets_the_documented_defaults() {
    let c = cfg("version: 1\n");
    assert_eq!((c.proxy.socket.as_deref(), c.proxy.pool.as_deref()), (None, None));
    let env = std::collections::BTreeMap::from([("XDG_RUNTIME_DIR".to_string(), "/run/user/1".to_string())]);
    assert_eq!(c.proxy.socket_path(&env).unwrap(), std::path::PathBuf::from("/run/user/1/rafa/mcp/proxy.sock"));
    assert_eq!(c.proxy.pool_path(&env).unwrap(), std::path::PathBuf::from("/run/user/1/rafa/mcp/servers"));
    assert_eq!(c.proxy.separator, DEFAULT_SEPARATOR);
    assert!(c.servers.is_empty() && c.profiles.is_empty());
}

#[test]
fn proxy_keys_override_defaults() {
    let c = cfg("proxy:\n  socket: /run/p.sock\n  pool: /run/pool\n  namespace: { separator: _ }\n");
    assert_eq!((c.proxy.socket.as_deref(), c.proxy.pool.as_deref(), c.proxy.separator.as_str()), (Some("/run/p.sock"), Some("/run/pool"), "_"));
}

#[test]
fn servers_carry_prefix_rules_timeout_and_transport() {
    let c = cfg(r#"
servers:
  slack:
    prefix: chat
    include: ["search_*", history]
    exclude: ["*_delete", { annotations: { destructiveHint: true } }]
    rename: { conversations_history: history }
    describe: { search_messages: "Search Slack." }
    timeout: 60s
  scratch:
    transport: { stdio: { command: my-mcp, args: [--stdio], env: { A: b } } }
  remote:
    transport: { unix: /run/remote.sock }
"#);
    let slack = &c.servers["slack"];
    assert_eq!(slack.prefix.as_deref(), Some("chat"));
    assert_eq!(slack.timeout_secs, Some(60));
    assert!(slack.rules.visible(&Tool::named("search_messages")));
    assert!(!slack.rules.visible(&Tool::named("other")));
    assert!(!slack.rules.visible(&Tool::named("history_delete")));
    assert_eq!(slack.rules.rename["conversations_history"], "history");
    assert_eq!(slack.rules.describe["search_messages"], "Search Slack.");
    let destructive = Tool { annotations: mcp_filter::Annotations { destructive: Some(true), ..Default::default() }, ..Tool::named("search_x") };
    assert!(!slack.rules.visible(&destructive));
    assert_eq!(c.servers["scratch"].transport, Some(Transport::Stdio {
            command: "my-mcp".into(),
            args: vec!["--stdio".into()],
            env: [("A".to_string(), "b".to_string())].into(),
        }));
    assert_eq!(c.servers["remote"].transport, Some(Transport::Unix("/run/remote.sock".into())));
    assert_eq!(c.servers["slack"].transport, None);
}

#[test]
fn timeouts_accept_seconds_minutes_and_bare_numbers() {
    let c = cfg("servers:\n  a: { timeout: 2m }\n  b: { timeout: 45 }\n  c: { timeout: 90s }\n");
    assert_eq!([c.servers["a"].timeout_secs, c.servers["b"].timeout_secs, c.servers["c"].timeout_secs], [Some(120), Some(45), Some(90)]);
}

#[test]
fn profiles_select_servers_with_their_rules() {
    let c = cfg("profiles:\n  coding:\n    servers:\n      gtasks: { include: [\"list_*\"] }\n      web: {}\n    toon: true\n    timeout: 30\n");
    let p = &c.profiles["coding"];
    assert!(p.toon);
    assert_eq!(p.timeout_secs, Some(30));
    assert_eq!(p.servers.keys().collect::<Vec<_>>(), vec!["gtasks", "web"]);
    assert!(p.servers["gtasks"].visible(&Tool::named("list_tasks")));
    assert!(!p.servers["gtasks"].visible(&Tool::named("delete_task")));
}

#[test]
fn invalid_shapes_name_their_path() {
    let bad = |text: &str| from_value(&Layer::parse("t", text).unwrap().value).unwrap_err();
    assert!(matches!(bad("servers:\n  s: { include: 3 }\n"), McpConfigError::Invalid { path, .. } if path == "servers.s.include"));
    assert!(matches!(bad("servers:\n  s: { timeout: soon }\n"), McpConfigError::Invalid { path, .. } if path == "servers.s.timeout"));
    assert!(matches!(bad("servers:\n  s: { transport: { carrier-pigeon: 1 } }\n"), McpConfigError::Invalid { path, .. } if path == "servers.s.transport"));
    assert!(matches!(bad("servers:\n  s: { include: [\"re:(\"] }\n"), McpConfigError::Filter(_)));
    assert!(matches!(bad("servers:\n  s: { exclude: [{ annotations: { color: red } }] }\n"), McpConfigError::Invalid { .. }));
}

#[test]
fn inline_flags_build_a_profile_with_qualified_patterns() {
    let p = inline_profile(
        &["slack".into(), "gtasks".into()],
        &["slack:search_*".into(), "list_*".into()],
        &["gtasks:list_secret".into()],
        true,
        Some(30),
    )
    .unwrap();
    assert!(p.toon && p.timeout_secs == Some(30));
    let slack = &p.servers["slack"];
    let gtasks = &p.servers["gtasks"];
    assert!(slack.visible(&Tool::named("search_messages")) && slack.visible(&Tool::named("list_users")));
    assert!(!slack.visible(&Tool::named("post")));
    assert!(gtasks.visible(&Tool::named("list_tasks")) && !gtasks.visible(&Tool::named("search_x")));
    assert!(!gtasks.visible(&Tool::named("list_secret")));
}

#[test]
fn inline_profile_rejects_patterns_for_unlisted_servers() {
    let err = inline_profile(&["slack".into()], &["web:browser_*".into()], &[], false, None).unwrap_err();
    assert!(matches!(err, McpConfigError::Invalid { ref path, .. } if path == "--include"));
}

#[test]
fn prefix_defaults_to_the_server_name() {
    let c = cfg("servers:\n  slack: { prefix: chat }\n");
    assert_eq!(prefix_of("slack", c.servers.get("slack")), "chat");
    assert_eq!(prefix_of("gtasks", c.servers.get("gtasks")), "gtasks");
}

#[test]
fn every_shape_error_is_reported() {
    let bad = |text: &str| from_value(&Layer::parse("t", text).unwrap().value).unwrap_err().to_string();
    for (yaml, needle) in [
        ("proxy: 3\n", "proxy"),
        ("proxy: { socket: [a] }\n", "proxy.socket"),
        ("servers:\n  s: { rename: [a] }\n", "servers.s.rename"),
        ("servers:\n  s: { describe: { a: [b] } }\n", "servers.s.describe"),
        ("servers:\n  s: { timeout: true }\n", "servers.s.timeout"),
        ("servers:\n  s: { timeout: 3d }\n", "servers.s.timeout"),
        ("servers:\n  s: { timeout: -4 }\n", "servers.s.timeout"),
        ("servers:\n  s: { exclude: [{ annotations: { readOnlyHint: yes-please } }] }\n", "booleans"),
        ("servers:\n  s: { exclude: [{ annotations: 3 }] }\n", "hints"),
        ("servers:\n  s: { exclude: [{ name: x }] }\n", "annotations"),
        ("servers:\n  s: { exclude: [3] }\n", "pattern"),
        ("servers:\n  s: { include: { a: b } }\n", "list of patterns"),
        ("servers:\n  s: { transport: { stdio: { args: [x] } } }\n", "command"),
        ("servers:\n  s: { transport: { stdio: { command: c, args: [1] } } }\n", "servers.s.transport.stdio.args"),
        ("servers:\n  s: { prefix: [p] }\n", "servers.s.prefix"),
    ] {
        let err = bad(yaml);
        assert!(err.contains(needle), "{yaml:?} -> {err}");
    }
}

#[test]
fn null_entries_hours_and_every_hint_are_accepted() {
    let c = cfg("proxy:\nservers:\n  bare:\n  long: { timeout: 1h }\n  hints:\n    exclude:\n      - { annotations: { readOnlyHint: true, idempotentHint: false, openWorldHint: true } }\n    transport: { stdio: { command: c } }\nprofiles:\n  p: { servers: { bare: } }\n");
    assert_eq!(c.proxy.socket, None);
    assert!(c.servers["bare"].transport.is_none());
    assert_eq!(c.servers["long"].timeout_secs, Some(3600));
    let all = mcp_filter::Annotations { read_only: Some(true), idempotent: Some(false), open_world: Some(true), ..Default::default() };
    assert!(!c.servers["hints"].rules.visible(&Tool { annotations: all, ..Tool::named("x") }));
    assert!(c.servers["hints"].rules.visible(&Tool::named("y")));
    assert_eq!(c.servers["hints"].transport, Some(Transport::Stdio { command: "c".into(), args: vec![], env: Default::default() }));
    assert!(c.profiles["p"].servers["bare"].visible(&Tool::named("z")));
}

#[test]
fn a_server_whose_name_starts_with_re_is_still_a_qualifier() {
    let p = inline_profile(&["remote".into(), "web".into()], &["remote:ping".into(), "re:get_.*".into()], &[], false, None).unwrap();
    assert!(p.servers["remote"].visible(&Tool::named("ping")));
    assert!(!p.servers["web"].visible(&Tool::named("ping")), "remote:ping must not reach web");
    assert!(p.servers["web"].visible(&Tool::named("get_x")), "re: stays a regex for every server");
}
