//! OAuth 2.0 for the gworkspace binaries (installed-app flow, no local listener: the user
//! pastes the redirect URL). Secrets are rafa-config refs, never inline; only a short-lived
//! access token leaves this crate.
use std::time::{Duration, Instant};

use rafa_config::{SecretRef, Value};

/// Every scope any gworkspace binary needs, combined into one refresh token.
/// Google Keep is not among them: it needs domain-wide delegation.
pub const COMBINED_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/tasks",
    "https://www.googleapis.com/auth/documents.readonly",
    "https://www.googleapis.com/auth/spreadsheets",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/meetings.space.readonly",
    "https://www.googleapis.com/auth/drive",
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/gmail.compose",
];

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("{path}: {reason}")]
    Config { path: String, reason: String },
    #[error(transparent)]
    Secret(#[from] rafa_config::ConfigError),
    #[error("OAuth client JSON is invalid: {0}")]
    InvalidClient(String),
    #[error("no code= parameter in the redirect URL")]
    MissingCode,
    #[error("no refresh_token in the token response (consent again with prompt=consent)")]
    MissingRefreshToken,
    #[error("token endpoint: HTTP {status}: {body}")]
    Token { status: u16, body: String },
    #[error("HTTP: {0}")]
    Http(String),
    #[error("storing the refresh token: {0}")]
    Store(String),
}

/// The `auth:` section of ~/.config/rafa/gworkspace.yaml.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    /// The installed-app client JSON downloaded from the Google Cloud console.
    pub client: SecretRef,
    pub refresh_token: SecretRef,
    /// argv that receives a new refresh token on stdin (`gtasks auth`); none: printed instead.
    pub store_refresh_token: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uris: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

fn cfg_err(path: &str, reason: impl Into<String>) -> AuthError {
    AuthError::Config { path: path.to_string(), reason: reason.into() }
}

pub fn auth_config(value: Option<&Value>) -> Result<AuthConfig, AuthError> {
    let auth = value.filter(|v| v.is_mapping()).ok_or_else(|| cfg_err("auth", "missing: needs client and refresh_token secret refs"))?;
    let secret = |key: &str| -> Result<SecretRef, AuthError> {
        let path = format!("auth.{key}");
        let v = auth.get(key).ok_or_else(|| cfg_err(&path, "missing"))?;
        Ok(SecretRef::parse(v, &path)?)
    };
    let store_refresh_token = match auth.get("store_refresh_token") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_sequence()
                .and_then(|s| s.iter().map(|x| x.as_str().map(str::to_string)).collect::<Option<Vec<_>>>())
                .filter(|argv| !argv.is_empty())
                .ok_or_else(|| cfg_err("auth.store_refresh_token", "expected a command (a non-empty list of strings)"))?,
        ),
    };
    Ok(AuthConfig { client: secret("client")?, refresh_token: secret("refresh_token")?, store_refresh_token })
}

#[derive(serde::Deserialize)]
struct InstalledDocument {
    installed: Installed,
}

#[derive(serde::Deserialize)]
struct Installed {
    client_id: String,
    client_secret: String,
    redirect_uris: Vec<String>,
}

pub fn parse_client(document: &str) -> Result<OAuthClient, AuthError> {
    let doc: InstalledDocument = serde_json::from_str(document).map_err(|e| AuthError::InvalidClient(e.to_string()))?;
    let i = doc.installed;
    if i.client_id.trim().is_empty() {
        return Err(AuthError::InvalidClient("installed.client_id is empty".into()));
    }
    if i.client_secret.trim().is_empty() {
        return Err(AuthError::InvalidClient("installed.client_secret is empty".into()));
    }
    if i.redirect_uris.is_empty() {
        return Err(AuthError::InvalidClient("installed.redirect_uris is empty".into()));
    }
    Ok(OAuthClient { client_id: i.client_id, client_secret: i.client_secret, redirect_uris: i.redirect_uris })
}

pub fn consent_url(client: &OAuthClient, scopes: &[&str]) -> String {
    let mut url = url::Url::parse(AUTH_ENDPOINT).expect("valid endpoint");
    url.query_pairs_mut()
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", &client.redirect_uris[0])
        .append_pair("response_type", "code")
        .append_pair("scope", &scopes.join(" "))
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("include_granted_scopes", "true");
    url.to_string()
}

pub fn extract_code(redirect_url: &str) -> Result<String, AuthError> {
    let url = url::Url::parse(redirect_url.trim()).map_err(|_| AuthError::MissingCode)?;
    url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned()).filter(|c| !c.is_empty()).ok_or(AuthError::MissingCode)
}

/// The form for exchanging `code` (one per redirect URI to try) or refreshing.
pub fn code_form(client: &OAuthClient, code: &str, redirect_uri: &str) -> Vec<(&'static str, String)> {
    vec![
        ("code", code.to_string()),
        ("client_id", client.client_id.clone()),
        ("client_secret", client.client_secret.clone()),
        ("redirect_uri", redirect_uri.to_string()),
        ("grant_type", "authorization_code".to_string()),
    ]
}

pub fn refresh_form(client: &OAuthClient, refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client.client_id.clone()),
        ("client_secret", client.client_secret.clone()),
        ("refresh_token", refresh_token.to_string()),
    ]
}

/// An access token and when to stop using it (60 s before Google says it expires).
#[derive(Debug, Clone)]
pub struct Cached {
    pub token: String,
    pub until: Instant,
}

pub fn cache_entry(response: &TokenResponse, now: Instant) -> Cached {
    let lifetime = Duration::from_secs(response.expires_in.unwrap_or(3600));
    Cached { token: response.access_token.clone(), until: now + lifetime.saturating_sub(Duration::from_secs(60)) }
}

fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn run_command(argv: &[String]) -> Result<String, String> {
    let out = std::process::Command::new(&argv[0]).args(&argv[1..]).output().map_err(|e| format!("{}: {e}", argv[0]))?;
    if !out.status.success() {
        return Err(format!("{} exited {}: {}", argv[0], out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Effects: resolves the secrets, talks to the token endpoint, caches the access token.
pub struct Auth {
    pub config: AuthConfig,
    pub token_endpoint: String,
    http: reqwest::Client,
    cache: tokio::sync::Mutex<Option<Cached>>,
}

impl Auth {
    /// An error (not a panic) when TLS cannot be set up, e.g. no CA certificates.
    pub fn new(config: AuthConfig, token_endpoint: impl Into<String>) -> Result<Auth, AuthError> {
        let http = reqwest::Client::builder().build().map_err(|e| {
            AuthError::Http(format!("HTTP client: {e}: {}", std::error::Error::source(&e).map(|s| s.to_string()).unwrap_or_default()))
        })?;
        Ok(Auth { config, token_endpoint: token_endpoint.into(), http, cache: tokio::sync::Mutex::new(None) })
    }

    pub fn client(&self) -> Result<OAuthClient, AuthError> {
        parse_client(&self.config.client.resolve("auth.client", read_file, run_command)?)
    }

    async fn post(&self, form: &[(&'static str, String)]) -> Result<TokenResponse, AuthError> {
        let response = self.http.post(&self.token_endpoint).form(form).send().await.map_err(|e| AuthError::Http(e.to_string()))?;
        let status = response.status();
        let body = response.text().await.map_err(|e| AuthError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(AuthError::Token { status: status.as_u16(), body });
        }
        serde_json::from_str(&body).map_err(|e| AuthError::Http(format!("token response: {e}")))
    }

    /// A valid access token, refreshing only when the cached one is about to expire.
    pub async fn access_token(&self) -> Result<String, AuthError> {
        let mut cache = self.cache.lock().await;
        if let Some(c) = cache.as_ref().filter(|c| Instant::now() < c.until) {
            return Ok(c.token.clone());
        }
        let client = self.client()?;
        let refresh = self.config.refresh_token.resolve("auth.refresh_token", read_file, run_command)?;
        let response = self.post(&refresh_form(&client, &refresh)).await?;
        let entry = cache_entry(&response, Instant::now());
        let token = entry.token.clone();
        *cache = Some(entry);
        Ok(token)
    }

    /// Exchange a pasted redirect URL for tokens; stores the refresh token when configured.
    pub async fn exchange(&self, redirect_url: &str) -> Result<TokenResponse, AuthError> {
        let client = self.client()?;
        let code = extract_code(redirect_url)?;
        let mut last = None;
        for uri in &client.redirect_uris {
            match self.post(&code_form(&client, &code, uri)).await {
                Ok(response) => {
                    let refresh = response.refresh_token.clone().ok_or(AuthError::MissingRefreshToken)?;
                    if let Some(argv) = &self.config.store_refresh_token {
                        store(argv, &refresh)?;
                    }
                    *self.cache.lock().await = Some(cache_entry(&response, Instant::now()));
                    return Ok(response);
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or(AuthError::MissingCode))
    }
}

fn store(argv: &[String], secret: &str) -> Result<(), AuthError> {
    use std::io::Write;
    let mut child = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| AuthError::Store(format!("{}: {e}", argv[0])))?;
    child.stdin.take().expect("piped").write_all(format!("{secret}\n").as_bytes()).map_err(|e| AuthError::Store(e.to_string()))?;
    let status = child.wait().map_err(|e| AuthError::Store(e.to_string()))?;
    if !status.success() {
        return Err(AuthError::Store(format!("{} exited {status}", argv[0])));
    }
    Ok(())
}
