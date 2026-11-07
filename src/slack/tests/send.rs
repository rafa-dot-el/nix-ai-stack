use serde_json::json;
use slack::{
    config::from_value,
    policy::Action,
    send::{check_file, plan, Body, File, Options, Step},
};

fn cfg() -> slack::config::Config {
    from_value(None).unwrap()
}

fn opts() -> Options {
    Options { client_msg_id: "uuid".into(), ..Default::default() }
}

fn file(name: &str) -> File {
    File { path: format!("/share/{name}"), name: name.into(), size: 3, title: None, alt: None, snippet_type: None }
}

#[test]
fn live_text_and_markdown() {
    let steps = plan(&Body { text: Some("hi".into()), ..Default::default() }, "C1", Action::Live, &opts(), &cfg(), false).unwrap();
    assert_eq!(steps, vec![Step::Post(json!({"channel": "C1", "text": "hi", "unfurl_links": false, "unfurl_media": true}))]);
    let long = format!("{}\n", "x".repeat(7000)).repeat(2);
    let steps = plan(&Body { markdown: Some(long), ..Default::default() }, "C1", Action::Live, &opts(), &cfg(), false).unwrap();
    assert_eq!(steps.len(), 2, "split at the markdown limit");
    assert!(matches!(&steps[0], Step::Post(v) if v["blocks"][0]["type"] == "markdown"));
    let short = plan(&Body { markdown: Some("**b**".into()), ..Default::default() }, "C1", Action::Live, &opts(), &cfg(), false).unwrap();
    assert_eq!(short, vec![Step::Post(json!({"channel": "C1", "markdown_text": "**b**", "unfurl_links": false, "unfurl_media": true}))]);
}

#[test]
fn tables_follow_the_body_and_threads_carry_through() {
    let table = vec![vec!["a".to_string()], vec!["1".to_string()]];
    let o = Options { thread_ts: Some("1.2".into()), broadcast: true, ..opts() };
    let steps = plan(&Body { text: Some("report".into()), tables: vec![table], ..Default::default() }, "C1", Action::Live, &o, &cfg(), false).unwrap();
    assert_eq!(steps.len(), 2);
    let Step::Post(t) = &steps[1] else { panic!() };
    assert_eq!((t["blocks"][0]["type"].clone(), t["thread_ts"].clone(), t["reply_broadcast"].clone()), (json!("table"), json!("1.2"), json!(true)));
    assert_eq!(t["text"], "table", "a fallback text for notifications");
}

#[test]
fn files_are_shared_live_and_private_for_drafts() {
    let body = Body { text: Some("see attached".into()), files: vec![file("a.png")], ..Default::default() };
    let live = plan(&body, "C1", Action::Live, &opts(), &cfg(), false).unwrap();
    assert!(matches!(&live[0], Step::Upload { share: true, .. }));
    let draft = plan(&body, "C1", Action::Draft, &opts(), &cfg(), true).unwrap();
    assert!(matches!(&draft[0], Step::Upload { share: false, .. }), "nothing is shared before the draft exists");
    let Step::Draft(form) = &draft[1] else { panic!() };
    assert_eq!(form.iter().find(|(k, _)| k == "text").unwrap().1, "see attached");
}

#[test]
fn drafts_need_a_session_token_and_fit_one_message() {
    let body = Body { text: Some("t".into()), ..Default::default() };
    assert!(plan(&body, "C1", Action::Draft, &opts(), &cfg(), false).unwrap_err().contains("xoxc"), "no silent fallback to a live post");
    let two = Body { text: Some("t".into()), tables: vec![vec![vec!["h".into()], vec!["1".into()]]], ..Default::default() };
    assert!(plan(&two, "C1", Action::Draft, &opts(), &cfg(), true).unwrap_err().contains("one message"));
    assert!(plan(&body, "C1", Action::Deny, &opts(), &cfg(), true).unwrap_err().contains("denied"));
}

#[test]
fn scheduling() {
    let o = Options { post_at: Some(1_800_000_000), ..opts() };
    let steps = plan(&Body { text: Some("later".into()), ..Default::default() }, "C1", Action::Live, &o, &cfg(), false).unwrap();
    assert_eq!(steps, vec![Step::Schedule(json!({"channel": "C1", "text": "later", "post_at": 1_800_000_000, "unfurl_links": false, "unfurl_media": true}))]);
    let with_file = Body { text: Some("x".into()), files: vec![file("a")], ..Default::default() };
    assert!(plan(&with_file, "C1", Action::Live, &o, &cfg(), false).unwrap_err().contains("schedul"));
    assert!(plan(&Body { text: Some("x".into()), ..Default::default() }, "C1", Action::Draft, &o, &cfg(), true).unwrap_err().contains("schedul"));
}

#[test]
fn attachment_roots_and_limits() {
    let roots = vec![std::path::PathBuf::from("/home/u/share")];
    assert!(check_file(std::path::Path::new("/home/u/share/a/b.png"), 10, &roots, 100).is_ok());
    assert!(check_file(std::path::Path::new("/home/u/shared/x"), 10, &roots, 100).unwrap_err().contains("allowed_roots"), "a prefix of a name is not a parent");
    assert!(check_file(std::path::Path::new("/etc/passwd"), 10, &roots, 100).is_err());
    assert!(check_file(std::path::Path::new("/home/u/share/big"), 101, &roots, 100).unwrap_err().contains("100"));
    assert!(check_file(std::path::Path::new("/home/u/share/a"), 1, &[], 100).unwrap_err().contains("allowed_roots"), "no roots: nothing may be attached");
}
