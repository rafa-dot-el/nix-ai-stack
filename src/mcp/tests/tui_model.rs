use std::sync::Arc;

use mcp::tui::model::{App, Effect, Inventory, Key, Origin, Scope, Screen};
use rafa_config::Layer;
use rmcp::model::{Tool, ToolAnnotations};

fn tool(name: &str) -> Tool {
    Tool::new(name.to_string(), format!("{name}: a tool that does a thing with words"), Arc::new(serde_json::Map::new()))
}

fn destructive(name: &str) -> Tool {
    let mut t = tool(name);
    let mut a = ToolAnnotations::new();
    a.destructive_hint = Some(true);
    t.annotations = Some(a);
    t
}

fn inventory() -> Inventory {
    Inventory::from([
        ("slack".to_string(), Ok(vec![tool("search"), tool("post"), destructive("delete")])),
        ("mine".to_string(), Ok(vec![tool("a"), tool("b"), tool("c")])),
        ("pooled".to_string(), Ok(vec![tool("x")])),
        ("down".to_string(), Err("connect: refused".to_string())),
    ])
}

fn app(user: &str) -> App {
    let nix = Layer::parse("10-nix.yaml", "servers:\n  slack: { exclude: [{ annotations: { destructiveHint: true } }] }\nprofiles:\n  ops: { servers: { slack: {} } }\n").unwrap();
    App::new(inventory(), vec![nix], Layer::parse("mcp.yaml", user).unwrap())
}

fn saved(effect: Effect) -> Layer {
    match effect {
        Effect::Save(v) => Layer { origin: "saved".into(), value: v },
        other => panic!("expected a save, got {other:?}"),
    }
}

#[test]
fn servers_summarise_origin_counts_tokens_and_profiles() {
    let a = app("servers:\n  mine: { exclude: [c] }\nprofiles:\n  dev: { servers: { mine: {}, slack: { include: [search] } } }\n");
    let s = a.servers();
    let names: Vec<_> = s.iter().map(|s| (s.name.as_str(), s.origin)).collect();
    assert_eq!(names, vec![("down", Origin::Pool), ("mine", Origin::User), ("pooled", Origin::Pool), ("slack", Origin::Nix)]);
    let mine = &s[1];
    assert_eq!((mine.shown, mine.total), (2, 3));
    assert!(mine.savings.full > mine.savings.filtered && mine.savings.percent_saved > 30.0);
    assert_eq!(mine.profiles, vec!["dev".to_string()]);
    let slack = &s[3];
    assert_eq!((slack.shown, slack.total), (2, 3), "the destructive tool is hidden by the Nix rule");
    assert_eq!(slack.profiles, vec!["dev".to_string(), "ops".to_string()]);
    assert_eq!(s[0].error.as_deref(), Some("connect: refused"));
    assert_eq!(a.profiles(), vec![("dev".to_string(), Origin::User), ("ops".to_string(), Origin::Nix)]);
}

#[test]
fn tool_rows_show_exposed_names_hidden_tools_and_flags() {
    let a = app("servers:\n  mine: { prefix: m, exclude: [c], rename: { a: alpha } }\n");
    let rows = a.tools(&Scope::Server("mine".into()));
    let view: Vec<_> = rows.iter().map(|r| (r.original.as_str(), r.exposed.as_deref())).collect();
    assert_eq!(view, vec![("a", Some("m-alpha")), ("b", Some("m-b")), ("c", None)]);
    let slack = a.tools(&Scope::Server("slack".into()));
    assert!(slack.iter().any(|r| r.original == "delete" && r.destructive && r.exposed.is_none()));
    let p = app("profiles:\n  dev: { servers: { mine: { include: [b] } } }\n");
    let rows = p.tools(&Scope::Profile("dev".into(), "mine".into()));
    assert_eq!(rows.iter().filter(|r| r.exposed.is_some()).map(|r| r.original.as_str()).collect::<Vec<_>>(), vec!["b"]);
}

fn open(a: &mut App, server_index: usize) {
    a.screen = Screen::Servers;
    a.cursor = server_index;
    assert_eq!(a.update(Key::Enter), Effect::None);
}

#[test]
fn toggles_keep_exclude_mode_and_save_the_user_layer() {
    let mut a = app("servers:\n  mine: { exclude: [c] }\n");
    open(&mut a, 1);
    assert_eq!(a.screen, Screen::Tools(Scope::Server("mine".into())));
    a.update(Key::Toggle); // hide a
    a.update(Key::Down);
    a.update(Key::Down);
    a.update(Key::Toggle); // show c
    assert!(a.dirty);
    let out = saved(a.update(Key::Save));
    assert_eq!(out.value["servers"]["mine"]["exclude"], serde_yaml_ng::from_str::<serde_yaml_ng::Value>("[a]").unwrap());
    assert!(out.value["servers"]["mine"].get("include").is_none(), "still exclude-based");
    assert!(!a.dirty);
}

#[test]
fn toggles_keep_include_mode() {
    let mut a = app("servers:\n  mine: { include: [a, b] }\n");
    open(&mut a, 1);
    a.update(Key::Toggle); // a: listed literally -> dropped from include
    a.update(Key::Down);
    a.update(Key::Down);
    a.update(Key::Toggle); // c: hidden -> added to include
    let out = saved(a.update(Key::Save));
    assert_eq!(out.value["servers"]["mine"]["include"], serde_yaml_ng::from_str::<serde_yaml_ng::Value>("[b, c]").unwrap());
}

#[test]
fn a_pattern_hidden_tool_is_explained_not_flipped() {
    let mut a = app("servers:\n  mine: { exclude: [\"c*\"] }\n");
    open(&mut a, 1);
    a.update(Key::Down);
    a.update(Key::Down);
    a.update(Key::Toggle);
    assert!(!a.dirty);
    assert!(a.message.as_deref().unwrap().contains("c*"), "{:?}", a.message);
}

#[test]
fn nix_entries_are_locked_and_pool_servers_get_an_entry() {
    let mut a = app("");
    open(&mut a, 3); // slack, from Nix
    a.update(Key::Toggle);
    assert!(!a.dirty && a.message.as_deref().unwrap().contains("Nix"), "{:?}", a.message);
    open(&mut a, 2); // pooled: no entry yet
    a.update(Key::Toggle);
    let out = saved(a.update(Key::Save));
    assert_eq!(out.value["servers"]["pooled"]["exclude"], serde_yaml_ng::from_str::<serde_yaml_ng::Value>("[x]").unwrap());
}

#[test]
fn profiles_screen_edits_user_profiles_per_server() {
    let mut a = app("profiles:\n  dev: { servers: { mine: { include: [b] } } }\n");
    assert_eq!(a.update(Key::Tab), Effect::None);
    assert_eq!(a.screen, Screen::Profiles);
    a.cursor = 0;
    a.update(Key::Enter);
    assert_eq!(a.screen, Screen::Tools(Scope::Profile("dev".into(), "mine".into())));
    a.update(Key::Toggle); // a: hidden in an include profile -> include it
    let out = saved(a.update(Key::Save));
    assert_eq!(out.value["profiles"]["dev"]["servers"]["mine"]["include"], serde_yaml_ng::from_str::<serde_yaml_ng::Value>("[b, a]").unwrap());
    a.screen = Screen::Profiles;
    a.cursor = 1; // ops, Nix
    a.update(Key::Enter);
    a.update(Key::Toggle);
    assert!(a.message.as_deref().unwrap().contains("Nix"));
}

#[test]
fn navigation_is_bounded_and_back_returns() {
    let mut a = app("");
    a.update(Key::Up);
    assert_eq!(a.cursor, 0);
    for _ in 0..10 {
        a.update(Key::Down);
    }
    assert_eq!(a.cursor, 3);
    a.update(Key::Enter);
    assert!(matches!(a.screen, Screen::Tools(_)));
    a.update(Key::Back);
    assert_eq!(a.screen, Screen::Servers);
    open(&mut a, 0); // a server that could not be listed has no tools screen
    assert_eq!(a.screen, Screen::Servers);
    assert!(a.message.as_deref().unwrap().contains("refused"));
}

#[test]
fn quitting_with_unsaved_changes_needs_a_second_quit() {
    let mut a = app("servers:\n  mine: {}\n");
    open(&mut a, 1);
    a.update(Key::Toggle);
    assert_eq!(a.update(Key::Quit), Effect::None);
    assert!(a.message.as_deref().unwrap().contains("unsaved"));
    assert_eq!(a.update(Key::Quit), Effect::Quit);
    let mut clean = app("");
    assert_eq!(clean.update(Key::Quit), Effect::Quit);
}

#[test]
fn an_edit_that_would_break_the_config_is_refused_on_save() {
    // renaming onto another tool's exposed name collides once b is shown again
    let mut a = app("servers:\n  mine: { prefix: \"\", exclude: [b], rename: { a: b } }\n");
    open(&mut a, 1);
    a.update(Key::Down);
    a.update(Key::Toggle); // show b -> collision with a renamed to b
    match a.update(Key::Save) {
        Effect::None => assert!(a.message.as_deref().unwrap().contains("b"), "{:?}", a.message),
        other => panic!("a colliding config was saved: {other:?}"),
    }
}

#[test]
fn removing_the_last_include_hides_by_exclusion_instead() {
    let mut a = app("servers:\n  mine: { include: [a] }\n");
    open(&mut a, 1);
    a.update(Key::Toggle);
    let out = saved(a.update(Key::Save));
    assert!(out.value["servers"]["mine"].get("include").is_none());
    assert_eq!(out.value["servers"]["mine"]["exclude"], serde_yaml_ng::from_str::<serde_yaml_ng::Value>("[a]").unwrap());
}

#[test]
fn an_annotation_pattern_is_named_in_the_explanation() {
    let mut a = app("servers:\n  pooled: { exclude: [{ annotations: { readOnlyHint: false } }] }\n");
    a.inventory.insert("pooled".into(), Ok(vec![{
        let mut t = tool("w");
        let mut an = ToolAnnotations::new();
        an.read_only_hint = Some(false);
        t.annotations = Some(an);
        t
    }]));
    open(&mut a, 2);
    a.update(Key::Toggle);
    assert!(a.message.as_deref().unwrap().contains("readOnlyHint"), "{:?}", a.message);
}

#[test]
fn configured_servers_that_are_not_listed_say_so() {
    let a = app("servers:\n  ghost: { transport: { unix: /x.sock } }\n");
    let ghost = a.servers().into_iter().find(|s| s.name == "ghost").unwrap();
    assert_eq!(ghost.error.as_deref(), Some("not running"));
}

#[test]
fn profile_navigation_explanations_and_clean_saves() {
    let mut a = app("servers:\n  mine: { exclude: [c] }\nprofiles:\n  dev: { servers: { mine: {} } }\n");
    a.update(Key::Tab);
    for _ in 0..5 {
        a.update(Key::Down);
    }
    assert_eq!(a.cursor, 1, "dev/mine and ops/slack");
    a.cursor = 0;
    a.update(Key::Enter);
    assert_eq!(a.update(Key::Enter), Effect::None, "Enter on a tools screen does nothing");
    a.update(Key::Down);
    a.update(Key::Down);
    a.update(Key::Toggle); // c is hidden by the server's own rules
    assert!(a.message.as_deref().unwrap().contains("server mine's own rules"), "{:?}", a.message);
    a.update(Key::Back);
    assert_eq!(a.screen, Screen::Profiles);
    assert_eq!(a.update(Key::Save), Effect::None);
    assert_eq!(a.message.as_deref(), Some("nothing to save"));
}
