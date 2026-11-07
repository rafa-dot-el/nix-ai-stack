//! GuerrillaMail's public JSON API: GET ajax.php?f=<action>, a sid_token per session, no
//! account or key (https://www.guerrillamail.com/GuerrillaMailAPI.html).
use serde_json::Value;

pub const BASE: &str = "https://api.guerrillamail.com/ajax.php";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub email: String,
    pub sid_token: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Summary {
    pub mail_id: Value,
    pub mail_from: String,
    pub mail_subject: String,
    pub mail_date: String,
    #[serde(default)]
    pub mail_excerpt: String,
    #[serde(default)]
    pub mail_read: Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Message {
    pub mail_id: Value,
    pub mail_from: String,
    pub mail_subject: String,
    pub mail_date: String,
    pub mail_body: String,
}

/// A message id as text: a string as it is, a number in digits (pure).
pub fn id_text(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Whether the API marked a message read (1 or "1") (pure).
pub fn is_read(v: &Value) -> bool {
    v.as_u64() == Some(1) || v.as_str() == Some("1")
}

pub struct Client {
    http: reqwest::Client,
    base: String,
}

#[derive(serde::Deserialize)]
struct Address {
    email_addr: String,
    sid_token: String,
}

#[derive(serde::Deserialize)]
struct List {
    list: Vec<Summary>,
}

impl Client {
    pub fn new(base: &str) -> Result<Client, String> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("tmpmail/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client: {e}"))?;
        Ok(Client { http, base: base.to_string() })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, action: &str, query: &[(&str, &str)]) -> Result<T, String> {
        let mut q = vec![("f", action)];
        q.extend_from_slice(query);
        let resp = self.http.get(&self.base).query(&q).send().await.map_err(|e| format!("{action}: {e}"))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| format!("{action}: {e}"))?;
        if !status.is_success() {
            return Err(format!("{action}: HTTP {status}: {body}"));
        }
        serde_json::from_str(&body).map_err(|e| format!("{action}: unexpected reply ({e}): {body}"))
    }

    /// A new random inbox.
    pub async fn new_address(&self) -> Result<Session, String> {
        let a: Address = self.get("get_email_address", &[]).await?;
        Ok(Session { email: a.email_addr, sid_token: a.sid_token })
    }

    pub async fn list(&self, s: &Session, offset: u32) -> Result<Vec<Summary>, String> {
        let offset = offset.to_string();
        let l: List = self.get("get_email_list", &[("offset", &offset), ("sid_token", &s.sid_token)]).await?;
        Ok(l.list)
    }

    pub async fn fetch(&self, s: &Session, id: &str) -> Result<Message, String> {
        self.get("fetch_email", &[("email_id", id), ("sid_token", &s.sid_token)]).await
    }

    /// Destroy the address server-side; whether the server said so.
    pub async fn forget(&self, s: &Session) -> Result<bool, String> {
        let v: Value = self.get("forget_me", &[("email_addr", &s.email), ("sid_token", &s.sid_token)]).await?;
        Ok(v == Value::Bool(true))
    }
}
