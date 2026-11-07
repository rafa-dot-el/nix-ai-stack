use tmpmail::format::*;

#[test]
fn parses_all_format_aliases_case_insensitively() {
    assert_eq!("TEXT".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
    assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
    assert_eq!("YAML".parse::<OutputFormat>().unwrap(), OutputFormat::Yaml);
    assert_eq!("yml".parse::<OutputFormat>().unwrap(), OutputFormat::Yaml);
    assert!("xml".parse::<OutputFormat>().is_err());
    assert_eq!(OutputFormat::Yaml.to_string(), "yaml");
}

#[test]
fn wrap_and_truncate() {
    for line in wrap("word ".repeat(30).trim(), 20).lines() {
        assert!(line.chars().count() <= 20, "{line:?}");
    }
    assert_eq!(wrap("para one\n\npara two", 80), "para one\n\npara two\n");
    let t = truncate(&"a".repeat(90), 80);
    assert_eq!((t.chars().count(), t.ends_with('\u{2026}')), (80, true));
    assert_eq!(truncate("short", 80), "short");
}

#[test]
fn text_views() {
    assert!(identity_list_text(&[]).contains("no identities"));
    let ids = [IdentityView { email: "a@b.c".into(), created_at: 7 }];
    assert!(identity_list_text(&ids).contains("a@b.c") && identity_list_text(&ids).contains("created_at=7"));
    assert_eq!(identity_created_text(&ids[0]), "a@b.c\n");
    assert_eq!(message_list_text(&[]), "(inbox empty)\n");
    let m = |id: &str, read, excerpt: &str| MessageView { id: id.into(), from: "f@x".into(), subject: "S".into(), date: "d".into(), read, excerpt: excerpt.into() };
    let text = message_list_text(&[m("1", false, "first line\nsecond"), m("2", true, "")]);
    assert!(text.contains("[1] N") && text.contains("[2] R") && text.contains("      first line") && !text.contains("second"));
    let detail = message_detail_text(&MessageDetailView { id: "9".into(), from: "f".into(), subject: "s".into(), date: "d".into(), body: String::new() }, "hello");
    assert!(detail.starts_with("Id:      9\n") && detail.ends_with("\nhello\n"));
}

#[test]
fn render_formats() {
    let v = IdentityView { email: "a@b.com".into(), created_at: 42 };
    assert_eq!(render(OutputFormat::Text, &v, || "custom\n".into()).unwrap(), "custom\n");
    let json = render(OutputFormat::Json, &v, String::new).unwrap();
    assert!(json.contains("\"email\": \"a@b.com\"") && json.ends_with("}\n"));
    assert!(render(OutputFormat::Yaml, &v, String::new).unwrap().contains("email: a@b.com"));
}

#[test]
fn strip_html_removes_tags_and_decodes_entities() {
    assert_eq!(strip_html("<pre>Hello &amp; welcome.\r\n\r\nClick &laquo;here&raquo;.</pre>"), "Hello & welcome.\r\n\r\nClick \u{ab}here\u{bb}.");
    assert_eq!(strip_html(r#"<a href="http://example.com">link</a> and <b>bold</b>"#), "link and bold");
    assert_eq!(strip_html("no markup"), "no markup");
    assert_eq!(strip_html("&lt;b&gt; &quot;q&quot; &#039;s&#039;"), "<b> \"q\" 's'");
}

#[test]
fn an_escaped_entity_is_decoded_once() {
    // bug 08c8fdae: &amp; was decoded first, so the quoted text "&lt;" became "<"
    assert_eq!(strip_html("write &amp;lt;b&amp;gt; for bold"), "write &lt;b&gt; for bold");
}
