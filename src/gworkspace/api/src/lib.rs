//! Google Workspace REST clients. Every client takes its base URL, so tests point it at a
//! local stub; `Google::default_base(...)` gives the real one.
pub mod calendar;
pub mod docs;
pub mod drive;
pub mod gmail;
pub mod meet;
pub mod sheets;
pub mod tasks;

use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("{api} returned HTTP {status}: {body}")]
    Failed { api: String, status: u16, body: String },
}

/// A safe file stem from a remote title: no `/` `\` or control characters, no leading dots;
/// `fallback` when nothing is left (pure).
pub fn file_stem(title: &str, fallback: &str) -> String {
    let cleaned: String = title.trim().chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim_start_matches('.').trim().to_string();
    if cleaned.is_empty() { fallback.to_string() } else { cleaned }
}

/// What every client holds: the HTTP client, the access token and the API's base URL.
#[derive(Clone)]
pub struct Session {
    pub http: reqwest::Client,
    pub token: String,
    pub base: String,
}

fn http_err(e: reqwest::Error) -> ApiError {
    ApiError::Http(e.to_string())
}

/// An HTTP client; an error (not a panic) when TLS cannot be set up, e.g. no CA certificates.
pub fn http_client() -> Result<reqwest::Client, ApiError> {
    reqwest::Client::builder().build().map_err(|e| ApiError::Http(format!("HTTP client: {e}: {}", std::error::Error::source(&e).map(|s| s.to_string()).unwrap_or_default())))
}

impl Session {
    pub fn new(token: impl Into<String>, base: impl Into<String>) -> Result<Session, ApiError> {
        Ok(Session { http: http_client()?, token: token.into(), base: base.into() })
    }

    async fn parse<T: DeserializeOwned>(url: &url::Url, response: reqwest::Response) -> Result<T, ApiError> {
        let status = response.status();
        let body = response.text().await.map_err(http_err)?;
        if !status.is_success() {
            return Err(ApiError::Failed { api: url.path().to_string(), status: status.as_u16(), body });
        }
        serde_json::from_str(if body.is_empty() { "null" } else { &body })
            .map_err(|e| ApiError::Http(format!("{}: unexpected response: {e}", url.path())))
    }

    pub async fn get<T: DeserializeOwned>(&self, url: &url::Url) -> Result<T, ApiError> {
        let response = self.http.get(url.clone()).bearer_auth(&self.token).send().await.map_err(http_err)?;
        Self::parse(url, response).await
    }

    pub async fn send<T: DeserializeOwned, B: serde::Serialize>(&self, method: reqwest::Method, url: &url::Url, body: &B) -> Result<T, ApiError> {
        let response = self.http.request(method, url.clone()).bearer_auth(&self.token).json(body).send().await.map_err(http_err)?;
        Self::parse(url, response).await
    }

    pub async fn delete(&self, url: &url::Url) -> Result<(), ApiError> {
        let response = self.http.delete(url.clone()).bearer_auth(&self.token).send().await.map_err(http_err)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Failed { api: url.path().to_string(), status: status.as_u16(), body });
        }
        Ok(())
    }

    /// GET `url` page by page, collecting `field`; `pageToken` is query-encoded.
    pub async fn paged(&self, url: &url::Url, field: &str, cap: Option<usize>) -> Result<Vec<serde_json::Value>, ApiError> {
        let mut items = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut page = url.clone();
            if let Some(t) = &token {
                page.query_pairs_mut().append_pair("pageToken", t);
            }
            let response: serde_json::Value = self.get(&page).await?;
            if let Some(found) = response.get(field).and_then(|v| v.as_array()) {
                items.extend(found.iter().cloned());
            }
            if let Some(cap) = cap.filter(|c| items.len() >= *c) {
                items.truncate(cap);
                return Ok(items);
            }
            token = response.get("nextPageToken").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
            if token.is_none() {
                return Ok(items);
            }
        }
    }

    /// POST a raw body with its own Content-Type (multipart uploads).
    pub async fn post_bytes<T: DeserializeOwned>(&self, url: &url::Url, content_type: &str, body: Vec<u8>) -> Result<T, ApiError> {
        let response = self
            .http
            .post(url.clone())
            .bearer_auth(&self.token)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await
            .map_err(http_err)?;
        Self::parse(url, response).await
    }

    /// GET raw bytes (downloads, exports).
    pub async fn bytes(&self, url: &url::Url) -> Result<Vec<u8>, ApiError> {
        let response = self.http.get(url.clone()).bearer_auth(&self.token).send().await.map_err(http_err)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Failed { api: url.path().to_string(), status: status.as_u16(), body });
        }
        Ok(response.bytes().await.map_err(http_err)?.to_vec())
    }

    /// `base` joined with path segments, each percent-encoded.
    pub fn url(&self, segments: &[&str]) -> url::Url {
        let mut url = url::Url::parse(&self.base).expect("a valid base URL");
        url.path_segments_mut().expect("a base URL with a path").pop_if_empty().extend(segments);
        url
    }
}
