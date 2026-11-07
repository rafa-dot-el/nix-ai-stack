//! The Slack Web API over HTTP: form POSTs with a bearer token (and the `d` cookie for xoxc
//! session tokens), `ok: false` as errors, 429 retried after Retry-After, cursor paging.
use serde_json::Value;

pub const API: &str = "https://slack.com/api";
/// Retries of one call after 429 responses.
pub const MAX_RETRIES: u32 = 5;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SlackError {
    /// Slack's own error code (`invalid_auth`, `channel_not_found`, ...).
    #[error("{method}: {code}")]
    Api { method: String, code: String },
    #[error("{method}: rate limited {MAX_RETRIES} times in a row")]
    RateLimited { method: String },
    #[error("{0}")]
    Http(String),
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    token: String,
    cookie: Option<String>,
    pub base: String,
}

/// Percent-encode everything but RFC 3986 unreserved characters (Python's `quote(s, safe="")`).
pub fn quote(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn http_err(e: reqwest::Error) -> SlackError {
    SlackError::Http(format!("{e}: {}", std::error::Error::source(&e).map(|s| s.to_string()).unwrap_or_default()))
}

impl Client {
    /// An error (not a panic) when TLS cannot be set up.
    pub fn new(token: impl Into<String>, cookie: Option<String>, base: impl Into<String>) -> Result<Client, SlackError> {
        let http = reqwest::Client::builder().build().map_err(http_err)?;
        Ok(Client { http, token: token.into(), cookie, base: base.into() })
    }

    /// Session (xoxc) tokens, the only ones drafts.* accepts.
    pub fn is_session(&self) -> bool {
        self.token.starts_with("xoxc-")
    }

    async fn send(&self, method: &str, build: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder) -> Result<Value, SlackError> {
        let url = format!("{}/{method}", self.base.trim_end_matches('/'));
        for _ in 0..=MAX_RETRIES {
            let mut req = self.http.post(&url).bearer_auth(&self.token);
            if let (true, Some(cookie)) = (self.is_session(), &self.cookie) {
                req = req.header(reqwest::header::COOKIE, format!("d={}", quote(cookie)));
            }
            let response = build(req).send().await.map_err(http_err)?;
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
                tokio::time::sleep(std::time::Duration::from_secs(wait.min(60))).await;
                continue;
            }
            let status = response.status();
            let body: Value = response.json().await.map_err(|e| SlackError::Http(format!("{method}: HTTP {status}: {e}")))?;
            if body.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(body);
            }
            let code = body.get("error").and_then(Value::as_str).unwrap_or("unknown_error").to_string();
            return Err(SlackError::Api { method: method.to_string(), code });
        }
        Err(SlackError::RateLimited { method: method.to_string() })
    }

    pub async fn call(&self, method: &str, form: &[(&str, String)]) -> Result<Value, SlackError> {
        self.send(method, |r| r.form(form)).await
    }

    pub async fn call_json(&self, method: &str, body: &Value) -> Result<Value, SlackError> {
        self.send(method, |r| r.json(body)).await
    }

    /// POST raw bytes to an upload URL Slack handed out (files.getUploadURLExternal).
    pub async fn upload(&self, url: &str, bytes: Vec<u8>) -> Result<(), SlackError> {
        let response = self.http.post(url).body(bytes).send().await.map_err(http_err)?;
        if !response.status().is_success() {
            return Err(SlackError::Http(format!("upload: HTTP {}", response.status())));
        }
        Ok(())
    }

    /// Every page of `method` (cursor paging), collecting `field`, up to `max` items.
    pub async fn paged(&self, method: &str, form: &[(&str, String)], field: &str, max: usize) -> Result<Vec<Value>, SlackError> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        loop {
            let mut page: Vec<(&str, String)> = form.to_vec();
            if !cursor.is_empty() {
                page.push(("cursor", cursor.clone()));
            }
            let r = self.call(method, &page).await?;
            out.extend(r.get(field).and_then(Value::as_array).into_iter().flatten().cloned());
            if out.len() >= max {
                out.truncate(max);
                return Ok(out);
            }
            cursor = r.pointer("/response_metadata/next_cursor").and_then(Value::as_str).unwrap_or("").to_string();
            if cursor.is_empty() {
                return Ok(out);
            }
        }
    }
}
