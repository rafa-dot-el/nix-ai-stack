//! GET the OAuth usage endpoint, with an optional TTL cache for the exporter.
use std::{sync::Mutex, time::{Duration, Instant}};

use crate::model::UsageData;

pub const API_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const BETA_HEADER: &str = "oauth-2025-04-20";

pub struct Client {
    http: reqwest::Client,
    url: String,
    cache: Mutex<Option<(UsageData, Instant)>>,
}

impl Client {
    pub fn new(url: &str) -> Result<Client, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("claude-usage/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("HTTP client: {e}"))?;
        Ok(Client { http, url: url.to_string(), cache: Mutex::new(None) })
    }

    pub async fn fetch(&self, token: &str) -> Result<UsageData, String> {
        let resp = self
            .http
            .get(&self.url)
            .bearer_auth(token)
            .header("anthropic-beta", BETA_HEADER)
            .send()
            .await
            .map_err(|e| format!("usage API: {e}"))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| format!("usage API: {e}"))?;
        if !status.is_success() {
            return Err(format!("usage API: HTTP {status}: {body}"));
        }
        serde_json::from_str(&body).map_err(|e| format!("usage API: unexpected reply: {e}"))
    }

    /// `fetch`, answered from the last result while it is younger than `ttl`.
    pub async fn fetch_cached(&self, token: &str, ttl: Duration) -> Result<UsageData, String> {
        if let Some((data, at)) = self.cache.lock().expect("cache").as_ref() {
            if at.elapsed() < ttl {
                return Ok(data.clone());
            }
        }
        let data = self.fetch(token).await?;
        *self.cache.lock().expect("cache") = Some((data.clone(), Instant::now()));
        Ok(data)
    }
}
