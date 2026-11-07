use mcp::{
    config::{from_value, Transport},
    edit::{add_profile, add_server, remove, show, EditError, Kind, ProfileSpec, ServerSpec},
};
use mcp_filter::Tool;
use rafa_config::Layer;

fn nix() -> Vec<Layer> {
    vec![Layer::parse("10-nix.yaml", "version: 1\nservers:\n  slack: { prefix: chat }\nprofiles:\n  coding: { servers: { slack: {} } }\n").unwrap()]
}

fn user(text: &str) -> Layer {
    Layer::parse("mcp.yaml", text).unwrap()
}

fn typed(v: &rafa_config::Value) -> mcp::config::McpConfig {
    from_value(v).unwrap()
}

#[test]
fn add_server_writes_every_field_and_round_trips() {
    let spec = ServerSpec {
        transport: Some(Transport::Stdio { command: "my-mcp".into(), args: vec!["--stdio".into()], env: Default::default() }),
        prefix: Some("mine".into()),
        include: vec!["read_*".into()],
        exclude: vec!["read_secret".into()],
        describe: vec![("read_file".into(), "Read one file.".into())],
        timeout_secs: Some(30),
    };
    let v = add_server(&user("version: 1\n"), &nix(), "scratch", &spec).unwrap();
    let c = typed(&v);
    let s = &c.servers["scratch"];
    assert_eq!(s.prefix.as_deref(), Some("mine"));
    assert_eq!(s.timeout_secs, Some(30));
    assert!(s.rules.visible(&Tool::named("read_file")) && !s.rules.visible(&Tool::named("read_secret")));
    assert_eq!(s.rules.describe["read_file"], "Read one file.");
    assert_eq!(s.transport, spec.transport);
    assert_eq!(v["version"].as_i64(), Some(1), "untouched keys stay");
}

#[test]
fn add_server_with_a_unix_socket_or_only_rules() {
    let v = add_server(&user(""), &[], "remote", &ServerSpec { transport: Some(Transport::Unix("/run/r.sock".into())), ..Default::default() }).unwrap();
    assert_eq!(typed(&v).servers["remote"].transport, Some(Transport::Unix("/run/r.sock".into())));
    let v = add_server(&user(""), &[], "web", &ServerSpec { exclude: vec!["browser_install".into()], ..Default::default() }).unwrap();
    assert!(typed(&v).servers["web"].transport.is_none());
}

#[test]
fn existing_names_are_refused_with_their_origin() {
    let e = add_server(&user(""), &nix(), "slack", &ServerSpec::default()).unwrap_err();
    assert_eq!(e, EditError::Exists { kind: "server", name: "slack".into(), origin: "10-nix.yaml".into() });
    let e = add_server(&user("servers:\n  s: {}\n"), &[], "s", &ServerSpec::default()).unwrap_err();
    assert!(matches!(e, EditError::Exists { ref origin, .. } if origin == "mcp.yaml"));
}

#[test]
fn invalid_results_are_refused_before_writing() {
    let e = add_server(&user(""), &[], "bad", &ServerSpec { include: vec!["re:(".into()], ..Default::default() }).unwrap_err();
    assert!(matches!(e, EditError::Invalid(_)));
}

#[test]
fn add_profile_qualifies_patterns_per_server() {
    let spec = ProfileSpec {
        servers: vec!["slack".into(), "gtasks".into()],
        include: vec!["slack:search_*".into(), "list_*".into()],
        exclude: vec!["gtasks:list_secret".into()],
        toon: true,
        timeout_secs: Some(20),
    };
    let v = add_profile(&user(""), &nix(), "comms", &spec).unwrap();
    let p = &typed(&v).profiles["comms"];
    assert!(p.toon && p.timeout_secs == Some(20));
    assert!(p.servers["slack"].visible(&Tool::named("search_x")) && p.servers["slack"].visible(&Tool::named("list_users")));
    assert!(!p.servers["slack"].visible(&Tool::named("post")));
    assert!(!p.servers["gtasks"].visible(&Tool::named("list_secret")));
    let e = add_profile(&user(""), &[], "x", &ProfileSpec { servers: vec!["a".into()], include: vec!["b:y".into()], ..Default::default() }).unwrap_err();
    assert!(matches!(e, EditError::Invalid(_)));
    assert!(matches!(add_profile(&user(""), &nix(), "coding", &ProfileSpec::default()), Err(EditError::Exists { .. })));
    assert!(matches!(add_profile(&user(""), &[], "empty", &ProfileSpec::default()), Err(EditError::Invalid(_))));
}

#[test]
fn remove_touches_only_the_user_layer() {
    let u = user("servers:\n  mine: {}\nprofiles:\n  p: { servers: { mine: {} } }\n");
    let v = remove(&u, &nix(), Kind::Server, "mine").unwrap();
    assert!(!typed(&v).servers.contains_key("mine"));
    let v = remove(&u, &nix(), Kind::Profile, "p").unwrap();
    assert!(!typed(&v).profiles.contains_key("p"));
    assert_eq!(
        remove(&u, &nix(), Kind::Server, "slack").unwrap_err(),
        EditError::NotYours { kind: "server", name: "slack".into(), origin: "10-nix.yaml".into() }
    );
    assert!(matches!(remove(&u, &nix(), Kind::Profile, "nope"), Err(EditError::Missing { kind: "profile", .. })));
}

#[test]
fn show_merges_and_can_name_origins() {
    let layers = [nix(), vec![user("profiles:\n  mine: { servers: { slack: {} } }\n")]].concat();
    let yaml = show(&layers, false).unwrap();
    let back = Layer::parse("x", &yaml).unwrap();
    assert_eq!(back.value["servers"]["slack"]["prefix"].as_str(), Some("chat"));
    assert!(back.value["profiles"]["mine"].is_mapping());
    let origins = show(&layers, true).unwrap();
    assert!(origins.lines().any(|l| l == "servers.slack.prefix\t10-nix.yaml"), "{origins}");
    assert!(origins.lines().any(|l| l.starts_with("profiles.mine") && l.ends_with("\tmcp.yaml")), "{origins}");
    let dup = [nix(), nix()].concat();
    assert!(matches!(show(&dup, false), Err(EditError::Invalid(_))));
}

#[test]
fn stdio_env_is_written() {
    let spec = ServerSpec {
        transport: Some(Transport::Stdio { command: "c".into(), args: vec![], env: [("TOKEN_FILE".to_string(), "/run/t".to_string())].into() }),
        ..Default::default()
    };
    let v = add_server(&user(""), &[], "s", &spec).unwrap();
    assert_eq!(typed(&v).servers["s"].transport, spec.transport);
}
