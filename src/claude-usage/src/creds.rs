use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsFile {
    pub claude_ai_oauth: Option<OAuthData>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OAuthData {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    #[serde(default)]
    pub refresh_token_expires_at: Option<i64>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub subscription_type: Option<String>,
    #[serde(default)]
    pub rate_limit_tier: Option<String>,
}

pub fn read_credentials(path: &str) -> Result<CredentialsFile> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read credentials from {path}"))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse credentials from {path}"))
}

/// The access token when it is valid at `now` (Unix seconds) (pure). Claude Code owns the
/// credentials file and refreshes its own token; this tool never refreshes or writes it
/// (bug df9bc6a5: a refresh here rewrote the file without its other keys).
pub fn token_at(creds: &CredentialsFile, now: i64) -> Result<String> {
    let oauth = creds.claude_ai_oauth.as_ref().context("No claudeAiOauth section in credentials file")?;
    if now < oauth.expires_at {
        return Ok(oauth.access_token.clone());
    }
    anyhow::bail!("the access token expired: open Claude Code, which refreshes it (claude-usage only reads the file)")
}

/// Read the file and return a token valid now.
pub fn get_valid_token(credentials_path: &str) -> Result<String> {
    token_at(&read_credentials(credentials_path)?, chrono::Utc::now().timestamp())
}

pub fn subscription_type(credentials_path: &str) -> Option<String> {
    read_credentials(credentials_path)
        .ok()?
        .claude_ai_oauth?
        .subscription_type
}

pub fn rate_limit_tier(credentials_path: &str) -> Option<String> {
    read_credentials(credentials_path)
        .ok()?
        .claude_ai_oauth?
        .rate_limit_tier
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(name: &str) -> String {
        format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    // ---- CredentialsFile parsing ----

    #[test]
    fn parse_full_credentials() {
        let creds = read_credentials(&fixture_path("credentials_full.json")).unwrap();
        let oauth = creds.claude_ai_oauth.unwrap();
        assert_eq!(oauth.access_token, "test-access-token-abc123");
        assert_eq!(oauth.refresh_token, "test-refresh-token-xyz789");
        assert_eq!(oauth.expires_at, 9999999999);
        assert_eq!(oauth.refresh_token_expires_at, Some(9999999999));
        assert_eq!(oauth.subscription_type.as_deref(), Some("pro"));
        assert_eq!(oauth.rate_limit_tier.as_deref(), Some("default_tier"));
        assert!(oauth.scopes.is_some());
        let scopes = oauth.scopes.unwrap();
        assert!(scopes.contains(&"user:inference".to_string()));
        assert!(scopes.contains(&"user:profile".to_string()));
    }

    #[test]
    fn parse_minimal_credentials() {
        let creds = read_credentials(&fixture_path("credentials_minimal.json")).unwrap();
        let oauth = creds.claude_ai_oauth.unwrap();
        assert_eq!(oauth.access_token, "minimal-token");
        assert_eq!(oauth.refresh_token, "minimal-refresh");
        assert!(oauth.subscription_type.is_none());
        assert!(oauth.rate_limit_tier.is_none());
        assert!(oauth.scopes.is_none());
        assert!(oauth.refresh_token_expires_at.is_none());
    }

    #[test]
    fn parse_missing_oauth_section() {
        let json = r#"{"claudeAiOauth": null}"#;
        let tmp = std::env::temp_dir().join("mc_test_null_oauth.json");
        fs::write(&tmp, json).unwrap();
        let creds = read_credentials(tmp.to_str().unwrap()).unwrap();
        assert!(creds.claude_ai_oauth.is_none());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn parse_empty_object() {
        let json = r#"{}"#;
        let tmp = std::env::temp_dir().join("mc_test_empty.json");
        fs::write(&tmp, json).unwrap();
        let creds = read_credentials(tmp.to_str().unwrap()).unwrap();
        assert!(creds.claude_ai_oauth.is_none());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn parse_nonexistent_file() {
        let result = read_credentials("/tmp/definitely_does_not_exist_mc.json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_json() {
        let tmp = std::env::temp_dir().join("mc_test_bad_json.json");
        fs::write(&tmp, "not json at all").unwrap();
        let result = read_credentials(tmp.to_str().unwrap());
        assert!(result.is_err());
        let _ = fs::remove_file(&tmp);
    }

    // ---- get_valid_token ----

    #[test]
    fn valid_token_returned_when_not_expired() {
        let token = get_valid_token(&fixture_path("credentials_full.json")).unwrap();
        assert_eq!(token, "test-access-token-abc123");
    }

    #[test]
    fn valid_token_returned_for_minimal() {
        let token = get_valid_token(&fixture_path("credentials_minimal.json")).unwrap();
        assert_eq!(token, "minimal-token");
    }

    #[test]
    fn missing_oauth_section_errors() {
        let json = r#"{"claudeAiOauth": null}"#;
        let tmp = std::env::temp_dir().join("mc_test_get_null.json");
        fs::write(&tmp, json).unwrap();
        let result = get_valid_token(tmp.to_str().unwrap());
        assert!(result.is_err());
        assert!(format!("{:?}", result.unwrap_err()).contains("No claudeAiOauth"),);
        let _ = fs::remove_file(&tmp);
    }

    // ---- subscription_type / rate_limit_tier ----

    #[test]
    fn subscription_type_present() {
        assert_eq!(
            subscription_type(&fixture_path("credentials_full.json")),
            Some("pro".to_string())
        );
    }

    #[test]
    fn subscription_type_absent() {
        assert_eq!(
            subscription_type(&fixture_path("credentials_minimal.json")),
            None
        );
    }

    #[test]
    fn rate_limit_tier_present() {
        assert_eq!(
            rate_limit_tier(&fixture_path("credentials_full.json")),
            Some("default_tier".to_string())
        );
    }

    #[test]
    fn rate_limit_tier_absent() {
        assert_eq!(
            rate_limit_tier(&fixture_path("credentials_minimal.json")),
            None
        );
    }

    #[test]
    fn subscription_type_bad_path() {
        assert_eq!(subscription_type("/tmp/nope_mc.json"), None);
    }

    // ---- OAuthData serde ----

    #[test]
    fn oauth_data_unknown_fields_tolerated() {
        let json = r#"{
            "accessToken": "tok",
            "refreshToken": "ref",
            "expiresAt": 1000,
            "brandNewField": true,
            "anotherField": [1,2,3]
        }"#;
        let o: OAuthData = serde_json::from_str(json).unwrap();
        assert_eq!(o.access_token, "tok");
    }

    #[test]
    fn credentials_file_extra_fields_tolerated() {
        let json = r#"{
            "claudeAiOauth": {
                "accessToken": "tok",
                "refreshToken": "ref",
                "expiresAt": 1000
            },
            "someOtherProvider": {"key": "val"}
        }"#;
        let c: CredentialsFile = serde_json::from_str(json).unwrap();
        assert!(c.claude_ai_oauth.is_some());
    }
}
