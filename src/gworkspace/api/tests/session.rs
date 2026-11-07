mod common;

use gworkspace_api::{ApiError, Session};
use serde_json::json;

#[tokio::test]
async fn requests_carry_the_bearer_token_and_errors_their_status() {
    let s = common::stub(vec![(200, json!({"a": 1})), (404, json!({"error": "gone"}))]).await;
    let session = Session::new("tok", &s.base).unwrap();
    let v: serde_json::Value = session.get(&session.url(&["x"])).await.unwrap();
    assert_eq!(v, json!({"a": 1}));
    let err = session.get::<serde_json::Value>(&session.url(&["y"])).await.unwrap_err();
    assert!(matches!(err, ApiError::Failed { status: 404, ref body, .. } if body.contains("gone")), "{err}");
    let seen = s.seen();
    assert_eq!(seen[0].auth, "Bearer tok");
    assert_eq!((seen[0].method.as_str(), seen[0].target.as_str()), ("GET", "/x"));
}

#[tokio::test]
async fn url_segments_are_encoded() {
    let session = Session::new("t", "http://h/base/v1").unwrap();
    assert_eq!(session.url(&["lists", "a/b c", "tasks"]).as_str(), "http://h/base/v1/lists/a%2Fb%20c/tasks");
}

#[tokio::test]
async fn paging_follows_next_page_token_and_encodes_it() {
    let s = common::stub(vec![
        (200, json!({"items": [1, 2], "nextPageToken": "a+b/c="})),
        (200, json!({"items": [3]})),
    ])
    .await;
    let session = Session::new("t", &s.base).unwrap();
    let mut url = session.url(&["things"]);
    url.query_pairs_mut().append_pair("maxResults", "2");
    let all = session.paged(&url, "items", None).await.unwrap();
    assert_eq!(all, vec![json!(1), json!(2), json!(3)]);
    let second = &s.seen()[1].target;
    assert!(second.contains("pageToken=a%2Bb%2Fc%3D"), "the token must survive the query: {second}");
    assert!(second.contains("maxResults=2"), "{second}");
}

#[tokio::test]
async fn paging_stops_at_the_cap() {
    let s = common::stub(vec![(200, json!({"items": [1, 2, 3], "nextPageToken": "n"})), (200, json!({"items": [4]}))]).await;
    let session = Session::new("t", &s.base).unwrap();
    let all = session.paged(&session.url(&["t"]), "items", Some(2)).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(s.seen().len(), 1, "no second page once the cap is reached");
}

#[tokio::test]
async fn send_and_delete() {
    let s = common::stub(vec![(200, json!({"ok": true})), (204, json!(null))]).await;
    let session = Session::new("t", &s.base).unwrap();
    let v: serde_json::Value = session.send(reqwest::Method::PUT, &session.url(&["p"]), &json!({"k": "v"})).await.unwrap();
    assert_eq!(v["ok"], true);
    session.delete(&session.url(&["d"])).await.unwrap();
    let seen = s.seen();
    assert_eq!((seen[0].method.as_str(), seen[0].body.as_str()), ("PUT", r#"{"k":"v"}"#));
    assert_eq!(seen[1].method, "DELETE");
}

#[tokio::test]
async fn downloads_report_status_and_transport_errors() {
    let s = common::stub(vec![(404, json!({"error": "gone"}))]).await;
    let session = Session::new("t", &s.base).unwrap();
    let err = session.bytes(&session.url(&["f"])).await.unwrap_err();
    assert!(matches!(err, ApiError::Failed { status: 404, .. }), "{err}");
    let closed = Session::new("t", "http://127.0.0.1:1/x").unwrap();
    assert!(matches!(closed.get::<serde_json::Value>(&closed.url(&["y"])).await, Err(ApiError::Http(_))));
}
