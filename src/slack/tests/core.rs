use rafa_config::{Layer, SecretRef};
use slack::{
    config::{from_value, Format, Overflow, SLACK_MAX_FILE_BYTES},
    policy::{allowed, decide, Action, Destination, Match, Policy, Rule},
    target::{parse, permalink, ts_from_p, Permalink, Target},
};

#[test]
fn targets() {
    assert_eq!(parse("me").unwrap(), Target::Me);
    assert_eq!(parse("myself").unwrap(), Target::Me);
    assert_eq!(parse("@ada").unwrap(), Target::User("ada".into()));
    assert_eq!(parse("U0123ABC").unwrap(), Target::User("U0123ABC".into()));
    assert_eq!(parse("#team-reports").unwrap(), Target::ChannelName("team-reports".into()));
    assert_eq!(parse("notes").unwrap(), Target::ChannelName("notes".into()));
    assert_eq!(parse("C0123ABC").unwrap(), Target::ChannelId("C0123ABC".into()));
    assert_eq!(parse("D0123").unwrap(), Target::ChannelId("D0123".into()));
    for bad in ["", " ", "#", "@", "a b", "#x;y"] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn permalinks() {
    assert_eq!(ts_from_p("p1712345678123456").as_deref(), Some("1712345678.123456"));
    assert!(ts_from_p("p123").is_none() && ts_from_p("1712345678123456").is_none());
    assert_eq!(
        permalink("https://acme.slack.com/archives/C0123/p1712345678123456").unwrap(),
        Permalink { channel: "C0123".into(), ts: "1712345678.123456".into(), thread_ts: None }
    );
    assert_eq!(
        permalink("https://acme.slack.com/archives/C0123/p1712345678123456?thread_ts=1712345600.000100&cid=C0123").unwrap().thread_ts.as_deref(),
        Some("1712345600.000100")
    );
    for bad in ["https://evil.example/archives/C1/p1712345678123456", "https://acme.slack.com/messages/C1", "nope"] {
        assert!(permalink(bad).is_err(), "{bad}");
    }
}

fn policy() -> Policy {
    Policy {
        default: Action::Deny,
        rules: vec![
            Rule { matches: Match { self_dm: Some(true), ..Default::default() }, action: Action::Live },
            Rule { matches: Match { channel: Some("notes-*".into()), ..Default::default() }, action: Action::Live },
            Rule { matches: Match { channel: Some("team-*".into()), ..Default::default() }, action: Action::Draft },
            Rule { matches: Match { channel_id: Some("C0SECRET".into()), ..Default::default() }, action: Action::Deny },
        ],
    }
}

fn dest(id: &str, name: Option<&str>, self_dm: bool) -> Destination {
    Destination { channel_id: id.into(), name: name.map(Into::into), self_dm }
}

#[test]
fn first_matching_rule_wins_and_the_default_denies() {
    let p = policy();
    assert_eq!(decide(&p, &dest("D1", None, true)), (Action::Live, Some(0)));
    assert_eq!(decide(&p, &dest("C1", Some("notes-daily"), false)), (Action::Live, Some(1)));
    assert_eq!(decide(&p, &dest("C2", Some("team-reports"), false)), (Action::Draft, Some(2)));
    assert_eq!(decide(&p, &dest("C0SECRET", Some("general"), false)), (Action::Deny, Some(3)));
    assert_eq!(decide(&p, &dest("C3", Some("random"), false)), (Action::Deny, None));
    assert_eq!(decide(&p, &dest("D2", None, false)), (Action::Deny, None), "another person's DM");
    let all = Rule { matches: Match::default(), action: Action::Draft };
    assert_eq!(decide(&Policy { default: Action::Deny, rules: vec![all] }, &dest("X", None, false)), (Action::Draft, Some(0)), "an empty match matches all");
}

#[test]
fn a_request_may_ask_for_less_never_more() {
    assert_eq!(allowed(Action::Live, None).unwrap(), Action::Live);
    assert_eq!(allowed(Action::Live, Some(Action::Draft)).unwrap(), Action::Draft);
    assert!(allowed(Action::Draft, Some(Action::Live)).unwrap_err().contains("draft"));
    assert!(allowed(Action::Deny, None).unwrap_err().contains("denied"));
    assert!(allowed(Action::Deny, Some(Action::Draft)).is_err());
}

#[test]
fn config_defaults_deny_everything() {
    let c = from_value(None).unwrap();
    assert_eq!(c.policy.default, Action::Deny);
    assert!(c.policy.rules.is_empty() && c.token.is_none());
    assert_eq!((c.format, c.unfurl_links, c.unfurl_media, c.max_bytes, c.max_rows_per_message, c.overflow, c.tts), (Format::Markdown, false, true, SLACK_MAX_FILE_BYTES, 100, Overflow::Canvas, true));
}

#[test]
fn config_from_yaml() {
    let l = Layer::parse("slack.yaml", r#"
version: 1
credentials:
  token: { command: [pass, show, slack/token] }
  cookie: { file: /run/c }
defaults: { format: mrkdwn, unfurl: { links: true, media: false }, timezone: Europe/Warsaw }
policy:
  default: draft
  rules:
    - { match: { self_dm: true }, action: live }
    - { match: { channel: "notes-*" }, action: live }
attachments: { allowed_roots: ["/home/u/share"], max_bytes: 1048576 }
tables: { max_rows_per_message: 50, overflow: split }
tts: { enable: false }
"#).unwrap();
    let c = from_value(Some(&l.value)).unwrap();
    assert_eq!(c.token, Some(SecretRef::Command(vec!["pass".into(), "show".into(), "slack/token".into()])));
    assert_eq!(c.cookie, Some(SecretRef::File("/run/c".into())));
    assert_eq!((c.format, c.unfurl_links, c.unfurl_media, c.timezone.as_deref()), (Format::Mrkdwn, true, false, Some("Europe/Warsaw")));
    assert_eq!((c.policy.default, c.policy.rules.len()), (Action::Draft, 2));
    assert_eq!(c.policy.rules[1].matches.channel.as_deref(), Some("notes-*"));
    assert_eq!((c.allowed_roots.clone(), c.max_bytes, c.max_rows_per_message, c.overflow, c.tts), (vec!["/home/u/share".to_string()], 1048576, 50, Overflow::Split, false));
    let slack_max = Layer::parse("s", "attachments: { max_bytes: slack }\n").unwrap();
    assert_eq!(from_value(Some(&slack_max.value)).unwrap().max_bytes, SLACK_MAX_FILE_BYTES);
}

#[test]
fn config_errors_name_their_key() {
    let bad = |y: &str| from_value(Some(&Layer::parse("s", y).unwrap().value)).unwrap_err().to_string();
    assert!(bad("credentials: { token: xoxp-inline }\n").contains("credentials.token"));
    assert!(bad("policy: { default: maybe }\n").contains("policy.default"));
    assert!(bad("policy: { rules: [{ match: { colour: red }, action: live }] }\n").contains("policy.rules[0]"));
    assert!(bad("policy: { rules: [{ match: { channel: \"[\" }, action: live }] }\n").contains("policy.rules[0]"));
    assert!(bad("attachments: { max_bytes: lots }\n").contains("attachments.max_bytes"));
    assert!(bad("attachments: { max_bytes: 2000000000 }\n").contains("Slack"), "above Slack's own limit");
    assert!(bad("tables: { overflow: shrink }\n").contains("tables.overflow"));
    assert!(bad("defaults: { format: html }\n").contains("defaults.format"));
    assert!(bad("defaults: { format: [markdown] }\n").contains("expected a string"), "a list where a string belongs");
    assert!(bad("tts: { enable: maybe }\n").contains("tts.enable"), "not a boolean");
    assert!(bad("attachments: { max_bytes: -1 }\n").contains("positive"));
}
