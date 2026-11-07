mod common;

use claude_usage::client::Client;

#[tokio::test]
async fn the_call_carries_the_token_and_the_beta_header_and_is_cached() {
    let body: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/usage_full.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
    let s = common::stub(vec![(200, body.clone()), (200, body), (401, serde_json::json!({"error": "nope"}))]).await;
    let c = Client::new(&format!("{}/api/oauth/usage", s.base)).unwrap();
    assert!(c.fetch_cached("TOK", std::time::Duration::from_secs(60)).await.unwrap().five_hour.is_some());
    c.fetch_cached("TOK", std::time::Duration::from_secs(60)).await.unwrap();
    assert_eq!(s.seen().len(), 1, "the second call is answered from the cache");
    c.fetch_cached("TOK", std::time::Duration::ZERO).await.unwrap();
    let seen = s.seen();
    assert_eq!((seen.len(), seen[0].auth.as_str(), seen[0].target.as_str()), (2, "Bearer TOK", "/api/oauth/usage"));
    assert!(seen[0].head.to_lowercase().contains("anthropic-beta: oauth-2025-04-20"));
    let err = c.fetch("TOK").await.unwrap_err();
    assert!(err.contains("401"), "{err}");
}
