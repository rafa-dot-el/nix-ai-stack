mod common;

use base64::Engine;
use gworkspace_api::{gmail::{decode, draft_raw, message_view, Draft, Gmail}, Session};
use serde_json::json;

fn b64(s: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
}

fn message() -> serde_json::Value {
    json!({"id": "m1", "threadId": "t1", "snippet": "hi", "labelIds": ["INBOX"],
        "payload": {"mimeType": "multipart/mixed", "headers": [
            {"name": "From", "value": "A <a@x.org>"}, {"name": "to", "value": "b@y.org"}, {"name": "Subject", "value": "S"},
            {"name": "Message-ID", "value": "<id@x>"}],
        "parts": [
            {"mimeType": "multipart/alternative", "parts": [
                {"mimeType": "text/html", "body": {"data": b64("<p>html</p>")}},
                {"mimeType": "text/plain", "body": {"data": b64("plain body")}}]},
            {"mimeType": "application/pdf", "filename": "../../evil.pdf", "body": {"attachmentId": "A1", "size": 3}},
            {"mimeType": "image/png", "filename": "", "body": {"attachmentId": "INLINE"}}
        ]}})
}

#[test]
fn messages_are_viewed_with_text_and_attachments() {
    let v = message_view(&message());
    assert_eq!((v.from.as_deref(), v.to.as_deref(), v.subject.as_deref()), (Some("A <a@x.org>"), Some("b@y.org"), Some("S")));
    assert_eq!(v.body_text, "plain body");
    assert_eq!(v.message_id_header.as_deref(), Some("<id@x>"));
    assert_eq!(v.attachments.len(), 1, "unnamed inline parts are not attachments");
    assert_eq!(v.attachments[0].filename, "../../evil.pdf", "kept as named; sanitised only when written");
    assert_eq!(decode(&b64("x?y")), b"x?y");
    assert_eq!(decode("eD95"), b"x?y", "padded or not");
}

fn raw(d: &Draft) -> String {
    String::from_utf8(decode(&draft_raw(d).unwrap())).unwrap()
}

#[test]
fn drafts_are_well_formed() {
    let d = Draft { to: "b@y.org".into(), cc: Some("c@z.org".into()), subject: "Hello".into(), body: "Body\nline".into(), in_reply_to: Some("<id@x>".into()), ..Default::default() };
    let r = raw(&d);
    assert!(r.starts_with("To: b@y.org\r\nCc: c@z.org\r\nSubject: Hello\r\nIn-Reply-To: <id@x>\r\nReferences: <id@x>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\n"), "{r}");
    assert!(r.ends_with("\r\n\r\nBody\nline"), "{r}");
}

#[test]
fn headers_cannot_be_injected() {
    for d in [
        Draft { to: "b@y.org\r\nBcc: spy@evil.org".into(), subject: "s".into(), ..Default::default() },
        Draft { to: "b@y.org".into(), subject: "s\nBcc: spy@evil.org".into(), ..Default::default() },
        Draft { to: "b@y.org".into(), subject: "s".into(), cc: Some("c\r".into()), ..Default::default() },
        Draft { to: "b@y.org".into(), subject: "s".into(), in_reply_to: Some("<x>\nX-Evil: 1".into()), ..Default::default() },
    ] {
        let err = draft_raw(&d).unwrap_err();
        assert!(err.contains("line break"), "{err}");
    }
    assert!(draft_raw(&Draft { to: " ".into(), subject: "s".into(), ..Default::default() }).is_err(), "a recipient is required");
}

#[test]
fn non_ascii_subjects_are_encoded() {
    let r = raw(&Draft { to: "b@y.org".into(), subject: "Zażółć gęślą".into(), body: "ł".into(), ..Default::default() });
    let line = r.lines().find(|l| l.starts_with("Subject: ")).unwrap();
    assert_eq!(line, format!("Subject: =?UTF-8?B?{}?=", base64::engine::general_purpose::STANDARD.encode("Zażółć gęślą")));
}

#[tokio::test]
async fn threads_messages_attachments_drafts_labels() {
    let s = common::stub(vec![
        (200, json!({"threads": [{"id": "t1", "snippet": "s"}, {"snippet": "no id"}]})),
        (200, json!({"id": "t1", "messages": [message()]})),
        (200, message()),
        (200, json!({"raw": b64("From: a\r\n\r\nbody")})),
        (200, json!({"data": b64("PDF")})),
        (200, json!({"drafts": [{"id": "d1", "message": {"id": "m9", "threadId": "t9"}}]})),
        (200, json!({"id": "d2", "message": {"id": "m10", "threadId": "t1"}})),
        (200, json!({"labels": [{"id": "L", "name": "Work", "type": "user"}]})),
        (200, json!({"id": "L2", "name": "New"})),
        (200, json!({"id": "m1", "labelIds": ["L2"]})),
    ])
    .await;
    let g = Gmail { session: Session::new("t", format!("{}/gmail/v1/users/me", s.base)).unwrap() };
    assert_eq!(g.threads("from:a newer_than:7d", 500).await.unwrap().len(), 1);
    assert_eq!(g.thread("t1").await.unwrap().messages[0].body_text, "plain body");
    let (view, bytes) = g.message("m1").await.unwrap();
    assert_eq!((view.id.as_str(), bytes.as_slice()), ("m1", b"From: a\r\n\r\nbody".as_slice()));
    assert_eq!(g.attachment("m1", "A1").await.unwrap(), b"PDF");
    assert_eq!(g.drafts(10).await.unwrap()[0].thread_id.as_deref(), Some("t9"));
    let d = g.create_draft(&Draft { to: "b@y.org".into(), subject: "s".into(), body: "b".into(), ..Default::default() }, Some("t1")).await.unwrap();
    assert_eq!(d.id, "d2");
    assert_eq!(g.labels().await.unwrap()[0].name, "Work");
    assert_eq!(g.create_label("New").await.unwrap().id, "L2");
    g.modify_labels("messages", "m1", &["L2".into()], &["INBOX".into()]).await.unwrap();
    let seen = s.seen();
    let t: Vec<_> = seen.iter().map(|x| format!("{} {}", x.method, x.target)).collect();
    assert_eq!(t[0], "GET /gmail/v1/users/me/threads?q=from%3Aa+newer_than%3A7d&maxResults=100");
    assert_eq!(t[1], "GET /gmail/v1/users/me/threads/t1?format=full");
    assert_eq!(t[3], "GET /gmail/v1/users/me/messages/m1?format=raw");
    assert_eq!(t[4], "GET /gmail/v1/users/me/messages/m1/attachments/A1");
    assert_eq!(t[6], "POST /gmail/v1/users/me/drafts");
    let body: serde_json::Value = serde_json::from_str(&seen[6].body).unwrap();
    assert_eq!(body["message"]["threadId"], "t1");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[8].body).unwrap(), json!({"name": "New", "labelListVisibility": "labelShow", "messageListVisibility": "show"}));
    assert_eq!(t[9], "POST /gmail/v1/users/me/messages/m1/modify");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[9].body).unwrap(), json!({"addLabelIds": ["L2"], "removeLabelIds": ["INBOX"]}));
    assert!(g.modify_labels("drafts", "x", &[], &[]).await.is_err());
}
