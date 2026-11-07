use std::collections::BTreeMap;

use mcp_filter::{stale, Annotations, Catalog, FilterError, Matcher, Rules, Server, Tool};

fn tools(names: &[&str]) -> Vec<Tool> {
    names.iter().map(|n| Tool::named(n)).collect()
}

fn m(text: &str) -> Matcher {
    Matcher::parse(text).unwrap()
}

fn names(catalog: &Catalog) -> Vec<&str> {
    catalog.tools().iter().map(|t| t.name.as_str()).collect()
}

fn destructive(name: &str) -> Tool {
    Tool { annotations: Annotations { destructive: Some(true), ..Default::default() }, ..Tool::named(name) }
}

// -- Matcher --------------------------------------------------------------------------

#[test]
fn glob_matches_whole_names() {
    assert!(m("search_*").matches(&Tool::named("search_messages")));
    assert!(!m("search_*").matches(&Tool::named("xsearch_messages")));
    assert!(m("*_delete").matches(&Tool::named("file_delete")));
    assert!(m("history").matches(&Tool::named("history")));
    assert!(!m("history").matches(&Tool::named("history_all")));
}

#[test]
fn regex_needs_the_re_prefix_and_is_anchored() {
    let re = m("re:get_(task|list)");
    assert!(re.matches(&Tool::named("get_task")));
    assert!(!re.matches(&Tool::named("get_tasks")));
    assert!(!re.matches(&Tool::named("xget_task")));
}

#[test]
fn invalid_patterns_are_errors() {
    assert!(matches!(Matcher::parse("re:("), Err(FilterError::InvalidRegex { .. })));
    assert!(matches!(Matcher::parse("a[b"), Err(FilterError::InvalidGlob { .. })));
}

#[test]
fn annotation_pattern_matches_only_declared_equal_hints() {
    let pat = Matcher::annotations(Annotations { destructive: Some(true), ..Default::default() });
    assert!(pat.matches(&destructive("rm")));
    assert!(!pat.matches(&Tool::named("undeclared")));
    let ro = Tool { annotations: Annotations { destructive: Some(false), ..Default::default() }, ..Tool::named("ro") };
    assert!(!pat.matches(&ro));
}

#[test]
fn literal_is_only_a_wildcard_free_glob() {
    assert_eq!(m("history").literal(), Some("history"));
    assert_eq!(m("search_*").literal(), None);
    assert_eq!(m("re:history").literal(), None);
    assert_eq!(Matcher::annotations(Annotations::default()).literal(), None);
}

// -- Rules ----------------------------------------------------------------------------

#[test]
fn empty_include_means_everything_and_exclude_wins() {
    let rules = Rules { exclude: vec![m("*_delete")], ..Default::default() };
    assert!(rules.visible(&Tool::named("list")));
    assert!(!rules.visible(&Tool::named("task_delete")));

    let both = Rules { include: vec![m("task_*")], exclude: vec![m("task_delete")], ..Default::default() };
    assert!(both.visible(&Tool::named("task_list")));
    assert!(!both.visible(&Tool::named("task_delete")));
    assert!(!both.visible(&Tool::named("other")));
}

#[test]
fn exclude_by_annotation() {
    let rules = Rules {
        exclude: vec![Matcher::annotations(Annotations { destructive: Some(true), ..Default::default() })],
        ..Default::default()
    };
    assert!(!rules.visible(&destructive("drop")));
    assert!(rules.visible(&Tool::named("read")));
}

// -- Catalog --------------------------------------------------------------------------

#[test]
fn prefix_and_separator_name_every_tool() {
    let rules = Rules::default();
    let t = tools(&["search_messages", "history"]);
    let cat = Catalog::build(&[Server { name: "slack", prefix: "slack", rules: &rules, tools: &t }], None, "-").unwrap();
    assert_eq!(names(&cat), vec!["slack-history", "slack-search_messages"]);
    assert_eq!(cat.resolve("slack-history"), Some(("slack", "history")));
}

#[test]
fn empty_prefix_exposes_the_bare_name() {
    let rules = Rules::default();
    let t = tools(&["a"]);
    let cat = Catalog::build(&[Server { name: "s", prefix: "", rules: &rules, tools: &t }], None, "-").unwrap();
    assert_eq!(names(&cat), vec!["a"]);
}

#[test]
fn proxy_rename_is_prefixed_and_filters_still_use_the_original_name() {
    let rules = Rules {
        include: vec![m("conversations_*")],
        rename: BTreeMap::from([("conversations_history".into(), "history".into())]),
        ..Default::default()
    };
    let t = tools(&["conversations_history", "users_list"]);
    let cat = Catalog::build(&[Server { name: "slack", prefix: "slack", rules: &rules, tools: &t }], None, "-").unwrap();
    assert_eq!(names(&cat), vec!["slack-history"]);
    assert_eq!(cat.resolve("slack-history"), Some(("slack", "conversations_history")));
    // the original name of a renamed tool is not callable
    assert_eq!(cat.resolve("slack-conversations_history"), None);
    assert_eq!(cat.resolve("conversations_history"), None);
}

#[test]
fn descriptions_layer_client_over_proxy_over_server() {
    let mut search = Tool::named("search");
    search.description = Some("server text".into());
    let t = vec![search, Tool { description: Some("kept".into()), ..Tool::named("list") }];
    let proxy = Rules { describe: BTreeMap::from([("search".into(), "proxy text".into())]), ..Default::default() };
    let client = Rules { describe: BTreeMap::from([("list".into(), "client text".into())]), ..Default::default() };
    let profile = BTreeMap::from([("s".to_string(), client)]);
    let cat = Catalog::build(&[Server { name: "s", prefix: "s", rules: &proxy, tools: &t }], Some(&profile), "-").unwrap();
    let desc: Vec<_> = cat.tools().iter().map(|e| (e.name.as_str(), e.description.as_deref())).collect();
    assert_eq!(desc, vec![("s-list", Some("client text")), ("s-search", Some("proxy text"))]);
}

#[test]
fn profile_selects_servers_and_filters_on_original_names() {
    let none = Rules::default();
    let proxy = Rules { rename: BTreeMap::from([("search_messages".into(), "search".into())]), ..Default::default() };
    let slack = tools(&["search_messages", "post"]);
    let tasks = tools(&["list_tasks"]);
    let client = Rules { include: vec![m("search_*")], ..Default::default() };
    let profile = BTreeMap::from([("slack".to_string(), client)]);
    let servers = [
        Server { name: "slack", prefix: "slack", rules: &proxy, tools: &slack },
        Server { name: "gtasks", prefix: "tasks", rules: &none, tools: &tasks },
    ];
    let cat = Catalog::build(&servers, Some(&profile), "-").unwrap();
    assert_eq!(names(&cat), vec!["slack-search"]);
}

#[test]
fn client_rename_is_the_final_name_verbatim() {
    let none = Rules::default();
    let t = tools(&["search_messages"]);
    let client = Rules { rename: BTreeMap::from([("search_messages".into(), "find".into())]), ..Default::default() };
    let profile = BTreeMap::from([("slack".to_string(), client)]);
    let cat = Catalog::build(&[Server { name: "slack", prefix: "slack", rules: &none, tools: &t }], Some(&profile), "-").unwrap();
    assert_eq!(names(&cat), vec!["find"]);
    assert_eq!(cat.resolve("find"), Some(("slack", "search_messages")));
    assert_eq!(cat.resolve("slack-search_messages"), None);
}

#[test]
fn proxy_exclusion_cannot_be_undone_by_a_profile() {
    let proxy = Rules { exclude: vec![m("post")], ..Default::default() };
    let t = tools(&["post", "read"]);
    let client = Rules { include: vec![m("*")], ..Default::default() };
    let profile = BTreeMap::from([("s".to_string(), client)]);
    let cat = Catalog::build(&[Server { name: "s", prefix: "s", rules: &proxy, tools: &t }], Some(&profile), "-").unwrap();
    assert_eq!(names(&cat), vec!["s-read"]);
}

#[test]
fn collisions_are_errors_naming_both_sides() {
    let none = Rules::default();
    let a = tools(&["x"]);
    let b = tools(&["x"]);
    let servers = [
        Server { name: "one", prefix: "", rules: &none, tools: &a },
        Server { name: "two", prefix: "", rules: &none, tools: &b },
    ];
    assert_eq!(
        Catalog::build(&servers, None, "-").unwrap_err(),
        FilterError::Collision {
            name: "x".into(),
            first_server: "one".into(),
            first_tool: "x".into(),
            second_server: "two".into(),
            second_tool: "x".into(),
        }
    );
}

#[test]
fn rename_onto_another_tools_name_collides() {
    let rules = Rules { rename: BTreeMap::from([("a".into(), "b".into())]), ..Default::default() };
    let t = tools(&["a", "b"]);
    let err = Catalog::build(&[Server { name: "s", prefix: "", rules: &rules, tools: &t }], None, "-").unwrap_err();
    assert!(matches!(err, FilterError::Collision { ref name, .. } if name == "b"));
}

#[test]
fn hidden_and_unknown_names_resolve_to_nothing() {
    let rules = Rules { exclude: vec![m("secret")], ..Default::default() };
    let t = tools(&["secret", "open"]);
    let cat = Catalog::build(&[Server { name: "s", prefix: "s", rules: &rules, tools: &t }], None, "-").unwrap();
    assert_eq!(cat.resolve("s-secret"), None);
    assert_eq!(cat.resolve("nope"), None);
    assert_eq!(cat.resolve("s-open"), Some(("s", "open")));
}

#[test]
fn a_profile_naming_an_unknown_server_exposes_nothing_for_it() {
    let none = Rules::default();
    let t = tools(&["a"]);
    let profile = BTreeMap::from([("absent".to_string(), Rules::default())]);
    let cat = Catalog::build(&[Server { name: "s", prefix: "s", rules: &none, tools: &t }], Some(&profile), "-").unwrap();
    assert!(cat.tools().is_empty());
}

// -- stale ----------------------------------------------------------------------------

#[test]
fn stale_reports_literal_names_the_server_no_longer_offers() {
    let rules = Rules {
        include: vec![m("search_*"), m("gone_include")],
        exclude: vec![m("gone_exclude"), m("present")],
        rename: BTreeMap::from([("gone_rename".into(), "x".into()), ("present".into(), "y".into())]),
        describe: BTreeMap::from([("gone_describe".into(), "d".into())]),
    };
    let t = tools(&["present", "search_messages"]);
    assert_eq!(stale(&rules, &t), vec!["gone_describe", "gone_exclude", "gone_include", "gone_rename"]);
}
