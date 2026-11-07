mod common;

use gworkspace_api::{docs::{plain_text, structured_text, Docs}, drive::{query, quote, Drive, DOC_MIME}, Session};
use serde_json::json;

fn para(style: &str, text: &str) -> serde_json::Value {
    json!({"paragraph": {"paragraphStyle": {"namedStyleType": style}, "elements": [{"textRun": {"content": text}}]}})
}

fn body() -> Vec<serde_json::Value> {
    vec![
        para("HEADING_1", "Title\n"),
        para("NORMAL_TEXT", "Body text.\n"),
        json!({"table": {"tableRows": [{"tableCells": [{"content": [para("NORMAL_TEXT", "a\n")]}, {"content": [para("NORMAL_TEXT", "b\n")]}]}]}}),
        json!({"tableOfContents": {"content": [para("HEADING_2", "Part\n")]}}),
        para("HEADING_3", "  \n"),
    ]
}

#[test]
fn plain_text_concatenates_every_run() {
    assert_eq!(plain_text(&body()), "Title\nBody text.\na\nb\nPart\n  \n");
}

#[test]
fn structured_text_marks_headings_and_tables() {
    assert_eq!(structured_text(&body()), "# Title\nBody text.\na | b\n\n## Part\n  \n");
}

#[test]
fn drive_literals_escape_backslash_then_quote() {
    assert_eq!(quote("it's"), r"'it\'s'");
    assert_eq!(quote(r"a\"), r"'a\\'", "a trailing backslash must not escape the closing quote");
    assert_eq!(quote(r"x\'y"), r"'x\\\'y'");
    assert_eq!(query(DOC_MIME, None), "mimeType = 'application/vnd.google-apps.document' and trashed = false");
    assert_eq!(query(DOC_MIME, Some("q")), "mimeType = 'application/vnd.google-apps.document' and trashed = false and name contains 'q'");
    assert_eq!(query(DOC_MIME, Some("")), query(DOC_MIME, None));
}

#[tokio::test]
async fn get_create_append_delete_range() {
    let s = common::stub(vec![
        (200, json!({"documentId": "D", "title": "T", "body": {"content": body()}})),
        (200, json!({"documentId": "N", "title": "New"})),
        (200, json!({"body": {"content": [{"endIndex": 12}]}})),
        (200, json!({})),
        (200, json!({})),
    ])
    .await;
    let d = Docs { session: Session::new("t", format!("{}/v1/documents", s.base)).unwrap() };
    let doc = d.get("D", true).await.unwrap();
    assert_eq!((doc.document_id.as_deref(), doc.title.as_deref()), (Some("D"), Some("T")));
    assert!(doc.text.starts_with("# Title\n"));
    assert_eq!(d.create("New").await.unwrap().document_id.as_deref(), Some("N"));
    assert_eq!(d.append("D", "héllo 😀").await.unwrap(), (11, 11 + 8), "UTF-16 length");
    d.delete_range("D", 1, 5).await.unwrap();
    let seen = s.seen();
    assert_eq!(seen[1].method, "POST");
    assert_eq!(seen[3].target, "/v1/documents/D:batchUpdate");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[3].body).unwrap(), json!({"requests": [{"insertText": {"location": {"index": 11}, "text": "héllo 😀"}}]}));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[4].body).unwrap(), json!({"requests": [{"deleteContentRange": {"range": {"startIndex": 1, "endIndex": 5}}}]}));
}

#[tokio::test]
async fn a_document_id_cannot_rewrite_the_path() {
    let s = common::stub(vec![(200, json!({}))]).await;
    let d = Docs { session: Session::new("t", format!("{}/v1/documents", s.base)).unwrap() };
    d.delete_range("../../drive/v3/files/X?alt=media", 1, 2).await.unwrap();
    let target = &s.seen()[0].target;
    assert!(target.starts_with("/v1/documents/..%2F..%2Fdrive%2Fv3%2Ffiles%2FX%3Falt=media:batchUpdate"), "{target}");
}

#[tokio::test]
async fn drive_search_export_trash_delete() {
    let s = common::stub(vec![
        (200, json!({"files": [{"id": "1", "name": "A", "modifiedTime": "t"}, {"name": "no id"}]})),
        (200, json!("# md")),
        (200, json!({"id": "1", "name": "A", "trashed": true})),
        (204, json!(null)),
    ])
    .await;
    let d = Drive { session: Session::new("t", format!("{}/drive/v3/files", s.base)).unwrap() };
    let found = d.search(DOC_MIME, Some("it's"), 500).await.unwrap();
    assert_eq!(found.len(), 1, "entries without id/name are skipped");
    assert_eq!(d.export("1", "text/markdown").await.unwrap(), b"\"# md\"".to_vec());
    d.trash("1").await.unwrap();
    d.delete("1").await.unwrap();
    let seen = s.seen();
    let q: std::collections::BTreeMap<String, String> = url::Url::parse(&format!("http://h{}", seen[0].target)).unwrap().query_pairs().into_owned().collect();
    assert_eq!(q["q"], query(DOC_MIME, Some("it's")));
    assert_eq!(q["pageSize"], "100", "capped");
    assert_eq!(q["orderBy"], "modifiedTime desc");
    assert_eq!(seen[1].target, "/drive/v3/files/1/export?mimeType=text%2Fmarkdown");
    assert_eq!((seen[2].method.as_str(), seen[2].body.as_str()), ("PATCH", r#"{"trashed":true}"#));
    assert_eq!(seen[3].method, "DELETE");
}
