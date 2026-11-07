//! The exact requests (plan 5.6: matched against the Python's golden shapes, minus the
//! documented fixes). Pure: values in, form fields or JSON out.
use serde_json::Value;

/// One chat.postMessage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Post<'a> {
    pub channel: &'a str,
    /// Sent as markdown_text, alone (Slack refuses it with text or blocks).
    pub markdown: Option<&'a str>,
    pub text: Option<&'a str>,
    pub blocks: Option<&'a Value>,
    pub thread_ts: Option<&'a str>,
    pub broadcast: bool,
    pub unfurl_links: bool,
    pub unfurl_media: bool,
}

/// chat.postMessage (JSON body).
pub fn post_message(p: &Post<'_>) -> Value {
    let Post { channel, markdown, text, blocks, thread_ts, broadcast, unfurl_links, unfurl_media } = *p;
    let mut m = serde_json::json!({ "channel": channel });
    if let Some(md) = markdown {
        m["markdown_text"] = md.into();
    }
    if let Some(t) = text {
        m["text"] = t.into();
    }
    if let Some(b) = blocks {
        m["blocks"] = b.clone();
    }
    if let Some(ts) = thread_ts {
        m["thread_ts"] = ts.into();
        if broadcast {
            m["reply_broadcast"] = true.into();
        }
    }
    m["unfurl_links"] = unfurl_links.into();
    m["unfurl_media"] = unfurl_media.into();
    m
}

/// drafts.create (form fields, in order). Plain text becomes rich_text blocks (what the web
/// client saves); markdown goes as markdown_text alone (fix: not with text as well).
pub fn draft_form(channel: &str, client_msg_id: &str, text: Option<&str>, markdown: Option<&str>, blocks: Option<&Value>, thread_ts: Option<&str>, file_ids: &[String]) -> Vec<(String, String)> {
    let mut dest = serde_json::json!({ "channel_id": channel });
    if let Some(ts) = thread_ts {
        dest["message_ts"] = ts.into();
    }
    let mut f: Vec<(String, String)> = vec![
        ("channel".into(), channel.into()),
        ("client_msg_id".into(), client_msg_id.into()),
        ("destinations".into(), serde_json::json!([dest]).to_string()),
        ("file_ids".into(), serde_json::json!(file_ids).to_string()),
        ("is_from_composer".into(), "true".into()),
    ];
    if let Some(ts) = thread_ts {
        f.push(("thread_ts".into(), ts.into()));
    }
    if let Some(b) = blocks {
        f.push(("blocks".into(), b.to_string()));
    } else if let Some(md) = markdown {
        f.push(("markdown_text".into(), md.into()));
    } else if let Some(t) = text {
        let rich = serde_json::json!([{ "type": "rich_text", "elements": [{ "type": "rich_text_section", "elements": [{ "type": "text", "text": t }] }] }]);
        f.push(("blocks".into(), rich.to_string()));
        f.push(("text".into(), t.into()));
    }
    f
}

/// files.getUploadURLExternal (form).
pub fn upload_url_form(filename: &str, length: u64, alt: Option<&str>, snippet_type: Option<&str>) -> Vec<(String, String)> {
    let mut f = vec![("filename".to_string(), filename.to_string()), ("length".to_string(), length.to_string())];
    if let Some(a) = alt {
        f.push(("alt_txt".into(), a.into()));
    }
    if let Some(s) = snippet_type {
        f.push(("snippet_type".into(), s.into()));
    }
    f
}

/// files.completeUploadExternal (JSON). `channel`: share it there (live posts); `None`:
/// private (drafts: nothing is shared before the draft exists; fix of the old leak).
pub fn complete_upload(files: &[(String, String)], channel: Option<&str>, thread_ts: Option<&str>) -> Value {
    let list: Vec<Value> = files.iter().map(|(id, title)| serde_json::json!({ "id": id, "title": title })).collect();
    let mut v = serde_json::json!({ "files": list });
    if let Some(c) = channel {
        v["channel_id"] = c.into();
        if let Some(ts) = thread_ts {
            v["thread_ts"] = ts.into();
        }
    }
    v
}

/// The HTTP method for the bytes sent to upload_url (fix: POST, not PUT).
pub const UPLOAD_METHOD: &str = "POST";
