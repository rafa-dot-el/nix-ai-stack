use slack::{
    config::from_value,
    prepare::{post_at, prepare, thread_ts},
    spec::from_yaml,
};

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("slack-prep-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("share")).unwrap();
    d.canonicalize().unwrap()
}

fn cfg(root: &std::path::Path, extra: &str) -> slack::config::Config {
    let y = format!("attachments: {{ allowed_roots: [\"{}\"], max_bytes: 100 }}\n{extra}", root.display());
    from_value(Some(&rafa_config::Layer::parse("s", &y).unwrap().value)).unwrap()
}

#[test]
fn schedule_times() {
    assert_eq!(post_at("2026-12-01T09:00:00Z", None).unwrap(), 1_796_115_600);
    assert_eq!(post_at("2026-12-01 10:00", Some("Europe/Warsaw")).unwrap(), 1_796_115_600, "CET is UTC+1 in December");
    assert_eq!(post_at("2026-12-01 09:00", None).unwrap(), 1_796_115_600, "UTC by default");
    assert!(post_at("tomorrow 9", None).unwrap_err().contains("YYYY-MM-DD HH:MM"));
    assert!(post_at("2026-12-01 10:00", Some("Nowhere/X")).is_err());
}

#[test]
fn threads() {
    assert_eq!(thread_ts("https://acme.slack.com/archives/C1/p1712345678123456").unwrap(), "1712345678.123456");
    assert_eq!(thread_ts("https://acme.slack.com/archives/C1/p1712345678123456?thread_ts=1712345600.000100").unwrap(), "1712345600.000100", "a reply's link means its thread");
    assert_eq!(thread_ts("1712345678.123456").unwrap(), "1712345678.123456");
    assert!(thread_ts("yesterday").is_err());
}

#[test]
fn files_markdown_and_tables_are_read_inside_the_roots() {
    let d = dir("ok");
    let share = d.join("share");
    std::fs::write(share.join("r.md"), "# Report").unwrap();
    std::fs::write(share.join("t.csv"), "a,b\n1,2\n").unwrap();
    std::fs::write(share.join("img.png"), "png").unwrap();
    let spec = from_yaml(&format!(
        "to: me\nmarkdown_file: {0}/r.md\ntables: [{{ csv: {0}/t.csv }}]\nattach: [{{ path: {0}/img.png, as: image, alt: Chart }}]\nthread: \"1712345678.123456\"\nunfurl: {{ links: true }}\n",
        share.display()
    ))
    .unwrap();
    let (body, opts) = prepare(&spec, &cfg(&share, ""), &[], "u").unwrap();
    assert_eq!(body.markdown.as_deref(), Some("# Report"));
    assert_eq!(body.tables, vec![vec![vec!["a".to_string(), "b".to_string()], vec!["1".to_string(), "2".to_string()]]]);
    assert_eq!((body.files[0].name.as_str(), body.files[0].size, body.files[0].alt.as_deref()), ("img.png", 3, Some("Chart")));
    assert_eq!((opts.thread_ts.as_deref(), opts.unfurl_links, opts.client_msg_id.as_str()), (Some("1712345678.123456"), Some(true), "u"));
}

#[test]
fn anything_outside_the_roots_is_refused_including_through_symlinks() {
    let d = dir("out");
    std::fs::write(d.join("secret.txt"), "s").unwrap();
    std::os::unix::fs::symlink(d.join("secret.txt"), d.join("share/link.txt")).unwrap();
    let c = cfg(&d.join("share"), "");
    for path in [d.join("secret.txt"), d.join("share/link.txt"), d.join("share/../secret.txt")] {
        let spec = from_yaml(&format!("to: me\nattach: [{{ path: \"{}\" }}]\n", path.display())).unwrap();
        assert!(prepare(&spec, &c, &[], "u").unwrap_err().contains("allowed_roots"), "{}", path.display());
    }
    let md = from_yaml(&format!("to: me\nmarkdown_file: \"{}\"\n", d.join("secret.txt").display())).unwrap();
    assert!(prepare(&md, &c, &[], "u").unwrap_err().contains("allowed_roots"), "markdown files too");
    std::fs::write(d.join("share/big.bin"), vec![0u8; 101]).unwrap();
    let big = from_yaml(&format!("to: me\nattach: [{{ path: \"{}\" }}]\n", d.join("share/big.bin").display())).unwrap();
    assert!(prepare(&big, &c, &[], "u").unwrap_err().contains("100"));
}

#[test]
fn say_uses_the_shared_voice_and_can_be_off() {
    let d = dir("say");
    let fake = d.join("fake-voice");
    std::fs::write(&fake, "#!/bin/sh\nfor last; do :; done\ncat > \"$last\"\n").unwrap();
    std::os::unix::fs::PermissionsExt::set_mode(&mut std::fs::metadata(&fake).unwrap().permissions(), 0o755);
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let spec = from_yaml("to: me\nsay: hello there\n").unwrap();
    let tts = vec![fake.display().to_string(), "say".into(), "-".into(), "-o".into()];
    let (body, _) = prepare(&spec, &cfg(&d.join("share"), ""), &tts, "u").unwrap();
    assert_eq!(body.files.len(), 1);
    assert!(body.files[0].name.ends_with(".mp3"));
    assert_eq!(std::fs::read_to_string(&body.files[0].path).unwrap(), "hello there");
    assert!(prepare(&spec, &cfg(&d.join("share"), "tts: { enable: false }\n"), &tts, "u").unwrap_err().contains("tts"));
}

#[test]
fn schedules_are_parsed_into_options() {
    let d = dir("sched");
    let spec = from_yaml("to: me\ntext: later\nschedule: { at: \"2026-12-01 10:00\", tz: Europe/Warsaw }\n").unwrap();
    let (_, opts) = prepare(&spec, &cfg(&d.join("share"), ""), &[], "u").unwrap();
    assert_eq!(opts.post_at, Some(1_796_115_600));
}
