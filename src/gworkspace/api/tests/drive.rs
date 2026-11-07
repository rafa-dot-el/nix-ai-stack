mod common;

use gworkspace_api::{drive::{children_query, file_meta, multipart, name_query, Drive, FOLDER_MIME}, Session};
use serde_json::json;

fn drive(base: &str) -> Drive {
    Drive { session: Session::new("t", format!("{base}/drive/v3/files")).unwrap() }
}

fn f(id: &str, name: &str, mime: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "mimeType": mime, "size": "10", "parents": ["P"]})
}

#[test]
fn queries_quote_every_value() {
    assert_eq!(children_query(None), "trashed = false");
    assert_eq!(children_query(Some("x' or name contains '")), r"'x\' or name contains \'' in parents and trashed = false");
    assert_eq!(name_query(r"a\", None), r"name contains 'a\\' and trashed = false");
    assert_eq!(name_query("q", Some("text/plain' or '1")), r"name contains 'q' and trashed = false and mimeType = 'text/plain\' or \'1'");
}

#[test]
fn metadata_and_kinds() {
    let m = file_meta(&f("1", "n", FOLDER_MIME)).unwrap();
    assert!(m.is_folder() && !m.is_native());
    assert!(file_meta(&f("2", "d", "application/vnd.google-apps.document")).unwrap().is_native());
    assert!(!file_meta(&f("3", "p", "application/pdf")).unwrap().is_native());
    assert!(file_meta(&json!({"id": "x"})).is_none());
    assert_eq!(file_meta(&f("1", "n", "x")).unwrap().parents, vec!["P"]);
}

#[test]
fn multipart_bodies() {
    let body = multipart(&json!({"name": "a.txt", "parents": ["P"]}), "text/plain", b"hello", "B");
    assert_eq!(
        String::from_utf8(body).unwrap(),
        "--B\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{\"name\":\"a.txt\",\"parents\":[\"P\"]}\r\n--B\r\nContent-Type: text/plain\r\n\r\nhello\r\n--B--\r\n"
    );
}

#[tokio::test]
async fn upload_is_one_multipart_request() {
    let s = common::stub(vec![(200, f("N", "a.txt", "text/plain"))]).await;
    let d = drive(&s.base);
    let meta = d.upload("a.txt", "text/plain", b"hello world", Some("P")).await.unwrap();
    assert_eq!(meta.id, "N");
    let seen = s.seen();
    assert_eq!(seen.len(), 1, "no second request that could leave an orphan");
    assert!(seen[0].target.starts_with("/upload/drive/v3/files?uploadType=multipart&fields="), "{}", seen[0].target);
    assert!(seen[0].head.to_ascii_lowercase().contains("content-type: multipart/related; boundary="), "{}", seen[0].head);
    assert!(seen[0].body.contains(r#"{"name":"a.txt","parents":["P"]}"#) && seen[0].body.contains("hello world"), "{}", seen[0].body);
}

#[tokio::test]
async fn list_find_metadata_download_and_fetch() {
    let s = common::stub(vec![
        (200, json!({"files": [f("1", "a", "text/plain")], "nextPageToken": "n"})),
        (200, json!({"files": [f("2", "b", "text/plain")]})),
        (200, json!({"files": [f("3", "c", "text/plain")]})),
        (200, f("1", "a", "text/plain")),
        (200, json!("raw")),
        (200, json!("pdf")),
    ])
    .await;
    let d = drive(&s.base);
    assert_eq!(d.list(Some("P"), 5).await.unwrap().len(), 2, "pages until max or the end");
    assert_eq!(d.find("c", None, 5).await.unwrap()[0].id, "3");
    let m = d.metadata("1").await.unwrap();
    assert_eq!(d.fetch(&m, "application/pdf").await.unwrap(), b"\"raw\"");
    let native = file_meta(&f("9", "doc", "application/vnd.google-apps.document")).unwrap();
    assert_eq!(d.fetch(&native, "application/pdf").await.unwrap(), b"\"pdf\"");
    let folder = file_meta(&f("8", "dir", FOLDER_MIME)).unwrap();
    assert!(d.fetch(&folder, "x").await.unwrap_err().to_string().contains("folder"));
    let t: Vec<_> = s.seen().iter().map(|x| x.target.clone()).collect();
    assert!(t[0].contains("pageSize=5") && t[1].contains("pageToken=n"), "{t:?}");
    assert_eq!(t[4], "/drive/v3/files/1?alt=media");
    assert_eq!(t[5], "/drive/v3/files/9/export?mimeType=application%2Fpdf");
}

#[tokio::test]
async fn folders_moves_and_quota() {
    let s = common::stub(vec![
        (200, f("F", "dir", FOLDER_MIME)),
        (200, f("1", "a", "text/plain")),
        (200, f("1", "a", "text/plain")),
        (200, json!({"storageQuota": {"limit": "100", "usage": "40", "usageInDrive": "30", "usageInDriveTrash": "5"}})),
    ])
    .await;
    let d = drive(&s.base);
    assert!(d.create_folder("dir", Some("P")).await.unwrap().is_folder());
    d.move_to("1", "Q").await.unwrap();
    let q = d.quota().await.unwrap();
    assert_eq!((q.limit, q.usage, q.usage_in_drive, q.usage_in_drive_trash), (Some(100), 40, 30, 5));
    let seen = s.seen();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[0].body).unwrap(), json!({"name": "dir", "mimeType": FOLDER_MIME, "parents": ["P"]}));
    assert_eq!(seen[2].method, "PATCH");
    assert!(seen[2].target.contains("addParents=Q") && seen[2].target.contains("removeParents=P"), "{}", seen[2].target);
    assert_eq!(seen[3].target, "/drive/v3/about?fields=storageQuota");
}

#[tokio::test]
async fn paths_resolve_by_quoted_names_and_ambiguity_is_an_error() {
    let s = common::stub(vec![
        (200, json!({"files": [{"id": "A", "name": "it's"}]})),
        (200, json!({"files": [{"id": "B", "name": "x"}]})),
        (200, json!({"files": [{"id": "C1", "name": "dup"}, {"id": "C2", "name": "dup"}]})),
        (200, json!({"files": []})),
    ])
    .await;
    let d = drive(&s.base);
    assert_eq!(d.resolve_path("/it's/x/").await.unwrap(), "B");
    let q: std::collections::BTreeMap<String, String> = url::Url::parse(&format!("http://h{}", s.seen()[0].target)).unwrap().query_pairs().into_owned().collect();
    assert_eq!(q["q"], format!(r"name = 'it\'s' and 'root' in parents and mimeType = '{FOLDER_MIME}' and trashed = false"));
    assert!(d.resolve_path("dup").await.unwrap_err().to_string().contains("C1"));
    assert!(d.resolve_path("missing").await.unwrap_err().to_string().contains("missing"));
}

#[tokio::test]
async fn walks_page_and_visit_each_folder_once() {
    let s = common::stub(vec![
        (200, json!({"files": [f("D", "sub", FOLDER_MIME), f("1", "a", "text/plain")], "nextPageToken": "p"})),
        (200, json!({"files": [f("2", "b", "text/plain")]})),
        (200, json!({"files": [f("ROOT", "loop", FOLDER_MIME), f("3", "c", "text/plain")]})),
    ])
    .await;
    let d = drive(&s.base);
    let tree = d.walk("ROOT").await.unwrap();
    let paths: Vec<String> = tree.iter().map(|e| e.path.join("/")).collect();
    assert_eq!(paths, vec!["sub", "a", "b", "sub/loop", "sub/c"]);
    assert_eq!(s.seen().len(), 3, "ROOT is not walked again through the loop");
}
