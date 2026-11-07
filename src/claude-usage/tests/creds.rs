use claude_usage::creds::{read_credentials, token_at};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn a_valid_token_is_returned_and_an_expired_one_is_an_error_that_says_what_to_do() {
    let full = read_credentials(&fixture("credentials_full.json")).unwrap();
    assert_eq!(token_at(&full, 1_000).unwrap(), "test-access-token-abc123");
    let expired = read_credentials(&fixture("credentials_expired.json")).unwrap();
    let err = token_at(&expired, 9_999_999_999).unwrap_err().to_string();
    assert!(err.contains("expired") && err.contains("Claude Code"), "{err}");
    let none = serde_json::from_str("{}").unwrap();
    assert!(token_at(&none, 0).unwrap_err().to_string().contains("claudeAiOauth"));
}

#[test]
fn an_expired_token_leaves_claude_codes_file_untouched() {
    // bug df9bc6a5: the refresh rewrote the file with claudeAiOauth only (mcpOAuth was lost)
    let dir = std::env::temp_dir().join(format!("cu-creds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".credentials.json");
    let original = r#"{"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1},"mcpOAuth":{"srv":{"accessToken":"keep-me"}}}"#;
    std::fs::write(&path, original).unwrap();
    assert!(claude_usage::creds::get_valid_token(path.to_str().unwrap()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "byte-identical");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file either");
}
