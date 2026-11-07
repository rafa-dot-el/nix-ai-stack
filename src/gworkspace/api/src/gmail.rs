//! Gmail API v1: threads, messages for export, attachments, drafts, labels.
use serde::Serialize;
use serde_json::Value;

use crate::{ApiError, Session};

pub const BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadSummary {
    pub id: String,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attachment {
    pub attachment_id: String,
    /// As the sender named it: never a local path as is (see `file_stem`).
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MessageView {
    pub id: String,
    pub thread_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub date: Option<String>,
    pub message_id_header: Option<String>,
    pub labels: Vec<String>,
    pub snippet: Option<String>,
    pub body_text: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadView {
    pub id: String,
    pub messages: Vec<MessageView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DraftSummary {
    pub id: String,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub label_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub to: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub subject: String,
    pub body: String,
    /// Message-ID of the message replied to (sets In-Reply-To and References).
    pub in_reply_to: Option<String>,
}

const URL_SAFE_ANY: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    base64::engine::GeneralPurposeConfig::new().with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
);

/// Base64url in either padding form (pure).
pub fn decode(data: &str) -> Vec<u8> {
    use base64::Engine;
    URL_SAFE_ANY.decode(data.trim()).unwrap_or_default()
}

fn header(payload: &Value, name: &str) -> Option<String> {
    payload
        .get("headers")?
        .as_array()?
        .iter()
        .find(|h| h.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(name)))?
        .get("value")?
        .as_str()
        .map(str::to_string)
}

fn body_data(part: &Value) -> Option<String> {
    part.pointer("/body/data").and_then(Value::as_str).map(|d| String::from_utf8_lossy(&decode(d)).into_owned())
}

/// The first text/plain part, depth first; a body with no type counts as text.
fn body_text(part: &Value) -> String {
    let mime = part.get("mimeType").and_then(Value::as_str).unwrap_or("");
    if mime == "text/plain" {
        if let Some(text) = body_data(part) {
            return text;
        }
    }
    for child in part.get("parts").and_then(Value::as_array).into_iter().flatten() {
        let text = body_text(child);
        if !text.is_empty() {
            return text;
        }
    }
    if mime.is_empty() { body_data(part).unwrap_or_default() } else { String::new() }
}

fn attachments(part: &Value, out: &mut Vec<Attachment>) {
    let filename = part.get("filename").and_then(Value::as_str).unwrap_or("");
    if let (false, Some(id)) = (filename.is_empty(), part.pointer("/body/attachmentId").and_then(Value::as_str)) {
        out.push(Attachment {
            attachment_id: id.to_string(),
            filename: filename.to_string(),
            mime_type: part.get("mimeType").and_then(Value::as_str).unwrap_or("application/octet-stream").to_string(),
            size_bytes: part.pointer("/body/size").and_then(Value::as_u64).unwrap_or(0),
        });
    }
    for child in part.get("parts").and_then(Value::as_array).into_iter().flatten() {
        attachments(child, out);
    }
}

/// A message resource (format=full) as a view: headers, plain-text body, attachments (pure).
pub fn message_view(msg: &Value) -> MessageView {
    let payload = msg.get("payload").cloned().unwrap_or(Value::Null);
    let mut found = Vec::new();
    attachments(&payload, &mut found);
    let s = |k: &str| msg.get(k).and_then(Value::as_str).map(str::to_string);
    MessageView {
        id: s("id").unwrap_or_default(),
        thread_id: s("threadId"),
        from: header(&payload, "From"),
        to: header(&payload, "To"),
        cc: header(&payload, "Cc"),
        subject: header(&payload, "Subject"),
        date: header(&payload, "Date"),
        message_id_header: header(&payload, "Message-ID"),
        labels: msg.get("labelIds").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        snippet: s("snippet"),
        body_text: body_text(&payload),
        attachments: found,
    }
}

fn one_line<'a>(name: &str, value: &'a str) -> Result<&'a str, String> {
    if value.contains(['\r', '\n']) {
        return Err(format!("{name} contains a line break; refusing to build headers from it"));
    }
    Ok(value)
}

fn subject(value: &str) -> String {
    use base64::Engine;
    if value.is_ascii() { value.to_string() } else { format!("=?UTF-8?B?{}?=", base64::engine::general_purpose::STANDARD.encode(value)) }
}

/// The RFC 5322 message for a draft, base64url: header values with CR or LF are refused
/// (no injected headers), a non-ASCII subject is RFC 2047 encoded (pure).
pub fn draft_raw(d: &Draft) -> Result<String, String> {
    use base64::Engine;
    if d.to.trim().is_empty() {
        return Err("a draft needs a recipient (to)".into());
    }
    let mut raw = format!("To: {}\r\n", one_line("to", &d.to)?);
    if let Some(cc) = &d.cc {
        raw.push_str(&format!("Cc: {}\r\n", one_line("cc", cc)?));
    }
    if let Some(bcc) = &d.bcc {
        raw.push_str(&format!("Bcc: {}\r\n", one_line("bcc", bcc)?));
    }
    raw.push_str(&format!("Subject: {}\r\n", subject(one_line("subject", &d.subject)?)));
    if let Some(id) = &d.in_reply_to {
        let id = one_line("in_reply_to", id)?;
        raw.push_str(&format!("In-Reply-To: {id}\r\nReferences: {id}\r\n"));
    }
    raw.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\n\r\n");
    raw.push_str(&d.body);
    Ok(base64::engine::general_purpose::URL_SAFE.encode(raw))
}

fn draft_summary(v: &Value) -> Option<DraftSummary> {
    Some(DraftSummary {
        id: v.get("id")?.as_str()?.to_string(),
        message_id: v.pointer("/message/id").and_then(Value::as_str).map(str::to_string),
        thread_id: v.pointer("/message/threadId").and_then(Value::as_str).map(str::to_string),
    })
}

fn label(v: &Value) -> Option<Label> {
    Some(Label { id: v.get("id")?.as_str()?.to_string(), name: v.get("name")?.as_str()?.to_string(), label_type: v.get("type").and_then(Value::as_str).map(str::to_string) })
}

pub struct Gmail {
    pub session: Session,
}

impl Gmail {
    pub async fn threads(&self, query: &str, max: usize) -> Result<Vec<ThreadSummary>, ApiError> {
        let mut url = self.session.url(&["threads"]);
        url.query_pairs_mut().append_pair("q", query).append_pair("maxResults", &max.clamp(1, 100).to_string());
        let r: Value = self.session.get(&url).await?;
        Ok(r.get("threads")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| Some(ThreadSummary { id: t.get("id")?.as_str()?.to_string(), snippet: t.get("snippet").and_then(Value::as_str).map(str::to_string) }))
            .collect())
    }

    pub async fn thread(&self, id: &str) -> Result<ThreadView, ApiError> {
        let mut url = self.session.url(&["threads", id]);
        url.query_pairs_mut().append_pair("format", "full");
        let r: Value = self.session.get(&url).await?;
        Ok(ThreadView {
            id: r.get("id").and_then(Value::as_str).unwrap_or(id).to_string(),
            messages: r.get("messages").and_then(Value::as_array).into_iter().flatten().map(message_view).collect(),
        })
    }

    /// The message (format=full) and its raw RFC 5322 bytes (format=raw).
    pub async fn message(&self, id: &str) -> Result<(MessageView, Vec<u8>), ApiError> {
        let mut full = self.session.url(&["messages", id]);
        full.query_pairs_mut().append_pair("format", "full");
        let view = message_view(&self.session.get::<Value>(&full).await?);
        let mut raw = self.session.url(&["messages", id]);
        raw.query_pairs_mut().append_pair("format", "raw");
        let r: Value = self.session.get(&raw).await?;
        Ok((view, decode(r.get("raw").and_then(Value::as_str).unwrap_or(""))))
    }

    pub async fn attachment(&self, message: &str, id: &str) -> Result<Vec<u8>, ApiError> {
        let r: Value = self.session.get(&self.session.url(&["messages", message, "attachments", id])).await?;
        Ok(decode(r.get("data").and_then(Value::as_str).unwrap_or("")))
    }

    pub async fn drafts(&self, max: usize) -> Result<Vec<DraftSummary>, ApiError> {
        let mut url = self.session.url(&["drafts"]);
        url.query_pairs_mut().append_pair("maxResults", &max.clamp(1, 100).to_string());
        let r: Value = self.session.get(&url).await?;
        Ok(r.get("drafts").and_then(Value::as_array).into_iter().flatten().filter_map(draft_summary).collect())
    }

    pub async fn create_draft(&self, d: &Draft, thread: Option<&str>) -> Result<DraftSummary, ApiError> {
        let raw = draft_raw(d).map_err(|e| ApiError::Failed { api: "drafts".into(), status: 400, body: e })?;
        let mut message = serde_json::json!({ "raw": raw });
        if let Some(t) = thread {
            message["threadId"] = t.into();
        }
        let r: Value = self.session.send(reqwest::Method::POST, &self.session.url(&["drafts"]), &serde_json::json!({ "message": message })).await?;
        draft_summary(&r).ok_or_else(|| ApiError::Failed { api: "drafts".into(), status: 200, body: "draft without id".into() })
    }

    pub async fn labels(&self) -> Result<Vec<Label>, ApiError> {
        let r: Value = self.session.get(&self.session.url(&["labels"])).await?;
        Ok(r.get("labels").and_then(Value::as_array).into_iter().flatten().filter_map(label).collect())
    }

    pub async fn create_label(&self, name: &str) -> Result<Label, ApiError> {
        let body = serde_json::json!({ "name": name, "labelListVisibility": "labelShow", "messageListVisibility": "show" });
        let r: Value = self.session.send(reqwest::Method::POST, &self.session.url(&["labels"]), &body).await?;
        label(&r).ok_or_else(|| ApiError::Failed { api: "labels".into(), status: 200, body: "label without id or name".into() })
    }

    /// `kind`: "messages" or "threads".
    pub async fn modify_labels(&self, kind: &str, id: &str, add: &[String], remove: &[String]) -> Result<Value, ApiError> {
        if kind != "messages" && kind != "threads" {
            return Err(ApiError::Failed { api: kind.into(), status: 400, body: "labels are modified on messages or threads".into() });
        }
        let body = serde_json::json!({ "addLabelIds": add, "removeLabelIds": remove });
        self.session.send(reqwest::Method::POST, &self.session.url(&[kind, id, "modify"]), &body).await
    }
}
