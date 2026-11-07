//! Google Meet: meetings from Calendar events, conference records, participants,
//! transcripts and recordings from the Meet API v2, attachments and transcript search via
//! Drive. Resource names and meeting codes from callers are validated before use.
use serde::Serialize;
use serde_json::Value;

use crate::{ApiError, Session};

pub const BASE: &str = "https://meet.googleapis.com/v2";

/// Text of an HTML fragment: tags dropped, common entities decoded (pure).
pub fn strip_html(input: &str) -> String {
    let mut text = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text.trim();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after.find(';').filter(|&semi| semi <= 10).and_then(|semi| {
            let entity = &after[..semi];
            let ch = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
                e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            }?;
            Some((ch, semi))
        });
        match decoded {
            Some((ch, semi)) => {
                out.push(ch);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `conferenceRecords/<id>` with id of [A-Za-z0-9_-]; anything else is refused (pure).
pub fn record_name(name: &str) -> Result<&str, String> {
    let ok = name.strip_prefix("conferenceRecords/").is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    if ok { Ok(name) } else { Err(format!("{name:?} is not a conference record name (conferenceRecords/<id>)")) }
}

/// A Meet code like `abc-mnop-xyz` (letters, digits, dashes) (pure).
pub fn meeting_code(code: &str) -> Result<&str, String> {
    let ok = !code.is_empty() && code.len() <= 64 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok { Ok(code) } else { Err(format!("{code:?} is not a Meet code (letters, digits and dashes)")) }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeetingSummary {
    pub event_id: Option<String>,
    pub title: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub meet_url: Option<String>,
    pub meet_code: Option<String>,
    pub attendee_count: usize,
    pub has_attachments: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attendee {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub response: Option<String>,
    pub organizer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventAttachment {
    pub title: Option<String>,
    pub mime_type: Option<String>,
    pub file_id: Option<String>,
    pub file_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Meeting {
    pub event_id: Option<String>,
    pub title: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub duration_minutes: Option<i64>,
    pub meet_url: Option<String>,
    pub meet_code: Option<String>,
    /// HTML stripped.
    pub description: String,
    pub organizer: Option<String>,
    pub attendees: Vec<Attendee>,
    pub attachments: Vec<EventAttachment>,
}

pub fn summary(event: &Value) -> MeetingSummary {
    let s = |p: &str| event.pointer(p).and_then(Value::as_str).map(str::to_string);
    MeetingSummary {
        event_id: s("/id"),
        title: s("/summary").unwrap_or_else(|| "(no title)".into()),
        start: s("/start/dateTime").or_else(|| s("/start/date")),
        end: s("/end/dateTime").or_else(|| s("/end/date")),
        meet_url: s("/hangoutLink"),
        meet_code: s("/conferenceData/conferenceId"),
        attendee_count: event.get("attendees").and_then(Value::as_array).map_or(0, Vec::len),
        has_attachments: event.get("attachments").and_then(Value::as_array).is_some_and(|a| !a.is_empty()),
    }
}

pub fn meeting(event: &Value) -> Meeting {
    let base = summary(event);
    let duration_minutes = (|| {
        let a = chrono::DateTime::parse_from_rfc3339(base.start.as_deref()?).ok()?;
        let b = chrono::DateTime::parse_from_rfc3339(base.end.as_deref()?).ok()?;
        Some((b - a).num_minutes())
    })();
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Meeting {
        duration_minutes,
        description: strip_html(event.get("description").and_then(Value::as_str).unwrap_or("")),
        organizer: event.get("organizer").and_then(|o| s(o, "email")),
        attendees: event
            .get("attendees")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|a| Attendee { email: s(a, "email"), display_name: s(a, "displayName"), response: s(a, "responseStatus"), organizer: a.get("organizer").and_then(Value::as_bool).unwrap_or(false) })
            .collect(),
        attachments: event
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|a| EventAttachment { title: s(a, "title"), mime_type: s(a, "mimeType"), file_id: s(a, "fileId"), file_url: s(a, "fileUrl") })
            .collect(),
        event_id: base.event_id,
        title: base.title,
        start: base.start,
        end: base.end,
        meet_url: base.meet_url,
        meet_code: base.meet_code,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    pub name: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub transcripts: Vec<Value>,
    pub recordings: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Participant {
    pub name: Option<String>,
    /// signed_in, anonymous, phone or unknown.
    pub kind: String,
    pub display_name: Option<String>,
    pub earliest_start: Option<String>,
    pub latest_end: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub speaker: String,
    pub text: String,
    pub start_time: Option<String>,
}

pub struct Meet {
    pub session: Session,
}

fn bad(e: String) -> ApiError {
    ApiError::Failed { api: "meet".into(), status: 400, body: e }
}

/// A resource name from the API (`conferenceRecords/R/transcripts/T`) as checked segments.
fn segments(name: &str) -> Result<Vec<&str>, ApiError> {
    let parts: Vec<&str> = name.split('/').collect();
    if parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        Ok(parts)
    } else {
        Err(bad(format!("unexpected resource name {name:?}")))
    }
}

impl Meet {
    /// Every conference of a meeting code, with its transcripts and recordings.
    pub async fn records(&self, code: &str) -> Result<Vec<Record>, ApiError> {
        let code = meeting_code(code).map_err(bad)?;
        let mut url = self.session.url(&["conferenceRecords"]);
        url.query_pairs_mut().append_pair("filter", &format!("space.meeting_code=\"{code}\""));
        let mut out = Vec::new();
        for rec in self.session.paged(&url, "conferenceRecords", None).await? {
            let name = rec.get("name").and_then(Value::as_str).map(str::to_string);
            let (transcripts, recordings) = match &name {
                Some(n) => (self.transcripts(n).await?, self.recordings(n).await?),
                None => (Vec::new(), Vec::new()),
            };
            let s = |k: &str| rec.get(k).and_then(Value::as_str).map(str::to_string);
            out.push(Record { name, start_time: s("startTime"), end_time: s("endTime"), transcripts, recordings });
        }
        Ok(out)
}
    pub async fn participants(&self, record: &str) -> Result<Vec<Participant>, ApiError> {
        let record = record_name(record).map_err(bad)?;
        let mut parts = segments(record)?;
        parts.push("participants");
        let raw = self.session.paged(&self.session.url(&parts), "participants", None).await?;
        Ok(raw
            .iter()
            .map(|p| {
                let (kind, user) = [("signed_in", "signedinUser"), ("anonymous", "anonymousUser"), ("phone", "phoneUser")]
                    .iter()
                    .find_map(|(kind, key)| p.get(*key).map(|u| (kind.to_string(), u)))
                    .unwrap_or(("unknown".to_string(), &Value::Null));
                let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
                Participant { name: s("name"), kind, display_name: user.get("displayName").and_then(Value::as_str).map(str::to_string), earliest_start: s("earliestStartTime"), latest_end: s("latestEndTime") }
            })
            .collect())
}
    pub async fn transcripts(&self, record: &str) -> Result<Vec<Value>, ApiError> {
        let mut parts = segments(record_name(record).map_err(bad)?)?;
        parts.push("transcripts");
        self.session.paged(&self.session.url(&parts), "transcripts", None).await
}
    pub async fn recordings(&self, record: &str) -> Result<Vec<Value>, ApiError> {
        let mut parts = segments(record_name(record).map_err(bad)?)?;
        parts.push("recordings");
        self.session.paged(&self.session.url(&parts), "recordings", None).await
}
    /// The first transcript's entries, speakers named from the participants.
    pub async fn entries(&self, record: &str) -> Result<Vec<Entry>, ApiError> {
        let transcripts = self.transcripts(record).await?;
        let name = transcripts
            .first()
            .and_then(|t| t.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::Failed { api: record.into(), status: 404, body: "no transcript for this conference".into() })?
            .to_string();
        let speakers: std::collections::HashMap<String, String> = self
            .participants(record)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| Some((p.name?, p.display_name.unwrap_or_else(|| "Unknown".into()))))
            .collect();
        let mut parts = segments(&name)?;
        parts.push("entries");
        let raw = self.session.paged(&self.session.url(&parts), "transcriptEntries", None).await?;
        Ok(raw
            .iter()
            .map(|e| {
                let who = e.get("participant").and_then(Value::as_str).unwrap_or("");
                Entry {
                    speaker: speakers.get(who).cloned().unwrap_or_else(|| who.to_string()),
                    text: e.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                    start_time: e.get("startTime").and_then(Value::as_str).map(str::to_string),
                }
            })
            .collect())
}
}
