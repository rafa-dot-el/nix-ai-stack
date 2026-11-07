use serde_json::json;
use slack::{
    blocks::{markdown_messages, parse_csv, table_messages, MARKDOWN_MAX},
    spec::{from_json, from_yaml, validate, Spec},
    wire::{complete_upload, draft_form, post_message, upload_url_form, Post, UPLOAD_METHOD},
};

const SPEC: &str = r##"
to: "#team-reports"
title: Weekly report
markdown: |
  ## Summary
  Build is green.
tables:
  - csv: results.csv
attach:
  - { path: chart.png, as: image, alt: Build times }
  - { path: build.log, as: snippet, lang: text }
unfurl: { links: false, media: false }
thread: https://acme.slack.com/archives/C0123/p1712345678123456
broadcast: false
mode: draft
"##;

#[test]
fn yaml_and_json_specs_share_a_schema() {
    let s = from_yaml(SPEC).unwrap();
    assert_eq!((s.to.as_str(), s.title.as_deref(), s.tables.len(), s.attach.len()), ("#team-reports", Some("Weekly report"), 1, 2));
    assert_eq!(s.attach[0].kind.as_deref(), Some("image"));
    assert_eq!(s.unfurl.links, Some(false));
    let j = from_json(r#"{"to": "me", "text": "hi", "mode": "live"}"#).unwrap();
    assert_eq!((j.to.as_str(), j.text.as_deref()), ("me", Some("hi")));
    assert!(from_yaml("to: me\ncolour: red\n").unwrap_err().contains("colour"), "unknown keys are errors");
    assert!(from_json("{").is_err());
}

#[test]
fn unsafe_combinations_are_refused() {
    let base = Spec { to: "me".into(), text: Some("t".into()), ..Default::default() };
    assert!(validate(&base).is_ok());
    let two = Spec { markdown: Some("m".into()), ..base.clone() };
    assert!(validate(&two).unwrap_err().contains("one of"));
    let scheduled = Spec { schedule: Some(slack::spec::Schedule { at: "2026-12-01T09:00:00Z".into(), tz: None }), attach: vec![slack::spec::Attach { path: "a".into(), kind: None, alt: None, title: None, lang: None }], ..base.clone() };
    assert!(validate(&scheduled).unwrap_err().contains("schedul"));
    let lonely_broadcast = Spec { broadcast: true, ..base.clone() };
    assert!(validate(&lonely_broadcast).unwrap_err().contains("thread"));
    let empty = Spec { to: "me".into(), ..Default::default() };
    assert!(validate(&empty).unwrap_err().contains("nothing"));
    let bad_mode = Spec { mode: Some("loud".into()), ..base.clone() };
    assert!(validate(&bad_mode).unwrap_err().contains("mode"));
    let only_file = Spec { to: "me".into(), attach: vec![slack::spec::Attach { path: "a".into(), kind: Some("file".into()), alt: None, title: None, lang: None }], ..Default::default() };
    assert!(validate(&only_file).is_ok(), "attachments alone are a message");
    let bad_kind = Spec { attach: vec![slack::spec::Attach { path: "a".into(), kind: Some("movie".into()), alt: None, title: None, lang: None }], ..base };
    assert!(validate(&bad_kind).unwrap_err().contains("movie"));
}

#[test]
fn markdown_is_split_at_lines_within_the_limit() {
    assert_eq!(markdown_messages("# Hi\ntext"), vec![json!({"type": "markdown", "text": "# Hi\ntext"})]);
    let line = format!("{}\n", "x".repeat(999));
    let long = line.repeat(25);
    let parts = markdown_messages(&long);
    assert_eq!(parts.len(), 3);
    for p in &parts {
        let t = p["text"].as_str().unwrap();
        assert!(t.chars().count() <= MARKDOWN_MAX && !t.starts_with('\n'));
    }
    assert_eq!(parts.iter().map(|p| p["text"].as_str().unwrap()).collect::<String>(), long);
    let unbroken = "y".repeat(MARKDOWN_MAX + 5);
    assert_eq!(markdown_messages(&unbroken).len(), 2, "a line longer than the limit is cut");
}

#[test]
fn csv_parsing() {
    assert_eq!(parse_csv("a,b\r\n1,\"x, \"\"y\"\"\"\n").unwrap(), vec![vec!["a", "b"], vec!["1", "x, \"y\""]]);
    assert_eq!(parse_csv("a\n\"multi\nline\"\n").unwrap(), vec![vec!["a"], vec!["multi\nline"]]);
    assert!(parse_csv("\"unterminated\n").is_err());
    assert_eq!(parse_csv("").unwrap(), Vec::<Vec<String>>::new());
}

#[test]
fn tables_split_with_the_header_repeated() {
    let mut rows = vec![vec!["h1".to_string(), "h2".to_string()]];
    rows.extend((0..5).map(|i| vec![i.to_string(), format!("v{i}")]));
    let parts = table_messages(&rows, 3).unwrap();
    assert_eq!(parts.len(), 3, "5 data rows, 2 per part with the header");
    assert_eq!(parts[0], json!({"type": "table", "rows": [
        [{"type": "raw_text", "text": "h1"}, {"type": "raw_text", "text": "h2"}],
        [{"type": "raw_text", "text": "0"}, {"type": "raw_text", "text": "v0"}],
        [{"type": "raw_text", "text": "1"}, {"type": "raw_text", "text": "v1"}]]}));
    assert_eq!(parts[2]["rows"].as_array().unwrap().len(), 2);
    let wide = vec![(0..21).map(|i| i.to_string()).collect::<Vec<_>>()];
    assert!(table_messages(&wide, 100).unwrap_err().contains("20"));
    assert!(table_messages(&[], 100).unwrap().is_empty());
}

#[test]
fn post_message_payloads() {
    assert_eq!(post_message(&Post { channel: "C1", markdown: Some("**hi**"), unfurl_media: true, ..Default::default() }), json!({"channel": "C1", "markdown_text": "**hi**", "unfurl_links": false, "unfurl_media": true}));
    let blocks = json!([{"type": "markdown", "text": "x"}]);
    assert_eq!(
        post_message(&Post { channel: "C1", text: Some("fallback"), blocks: Some(&blocks), thread_ts: Some("1.2"), broadcast: true, unfurl_links: true, unfurl_media: false, markdown: None }),
        json!({"channel": "C1", "text": "fallback", "blocks": blocks, "thread_ts": "1.2", "reply_broadcast": true, "unfurl_links": true, "unfurl_media": false})
    );
}

fn form(fields: Vec<(String, String)>) -> Vec<(String, String)> {
    fields
}

#[test]
fn draft_forms_match_the_python_golden_shape() {
    // slack_send_draft.session_drafts.drafts_create(text=..., thread_ts=...) with a fixed uuid
    let f = form(draft_form("C1", "uuid-1", Some("plain body"), None, None, Some("1712345678.123456"), &[]));
    let keys: Vec<&str> = f.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["channel", "client_msg_id", "destinations", "file_ids", "is_from_composer", "thread_ts", "blocks", "text"]);
    let get = |k: &str| f.iter().find(|(x, _)| x == k).unwrap().1.clone();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&get("destinations")).unwrap(), serde_json::json!([{"channel_id": "C1", "message_ts": "1712345678.123456"}]));
    assert_eq!(get("file_ids"), "[]");
    assert_eq!(get("is_from_composer"), "true");
    // Compared as JSON: key order depends on serde_json's preserve_order, which toon enables
    // workspace-wide; Slack parses the field, so order carries no meaning.
    let blocks: serde_json::Value = serde_json::from_str(&get("blocks")).unwrap();
    assert_eq!(blocks, serde_json::json!([{"type": "rich_text", "elements": [{"type": "rich_text_section", "elements": [{"type": "text", "text": "plain body"}]}]}]));
    // fix 5.1: markdown_text alone, never with text
    let md = draft_form("C1", "u", None, Some("**b**"), None, None, &["F1".into()]);
    let mk: Vec<&str> = md.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(mk, vec!["channel", "client_msg_id", "destinations", "file_ids", "is_from_composer", "markdown_text"]);
    assert_eq!(md.iter().find(|(k, _)| k == "file_ids").unwrap().1, r#"["F1"]"#);
}

#[test]
fn uploads_post_and_stay_private_for_drafts() {
    assert_eq!(UPLOAD_METHOD, "POST", "fix 5.1: the old code used PUT");
    assert_eq!(upload_url_form("a.png", 42, Some("Build"), None), vec![("filename".to_string(), "a.png".to_string()), ("length".to_string(), "42".to_string()), ("alt_txt".to_string(), "Build".to_string())]);
    assert_eq!(upload_url_form("x.rs", 1, None, Some("rust")).last().unwrap(), &("snippet_type".to_string(), "rust".to_string()));
    let files = vec![("F1".to_string(), "a.png".to_string())];
    assert_eq!(complete_upload(&files, None, None), json!({"files": [{"id": "F1", "title": "a.png"}]}), "fix 5.1: no channel for drafts");
    assert_eq!(complete_upload(&files, Some("C1"), Some("1.2")), json!({"files": [{"id": "F1", "title": "a.png"}], "channel_id": "C1", "thread_ts": "1.2"}));
}
