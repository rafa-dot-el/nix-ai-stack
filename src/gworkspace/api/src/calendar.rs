//! Google Calendar API v3 (read-only): events in a window, one event, and pure renderers to
//! Org and iCalendar. The caller supplies "today" and the timestamp, so all of it is testable.
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use crate::{ApiError, Session};

pub const BASE: &str = "https://www.googleapis.com/calendar/v3";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub id: String,
    pub summary: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub all_day: bool,
    pub start_date: Option<NaiveDate>,
    pub end_date_exclusive: Option<NaiveDate>,
    pub start_utc: Option<DateTime<Utc>>,
    pub end_utc: Option<DateTime<Utc>>,
    pub recurring: bool,
}

fn summary(s: &str) -> String {
    let t: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let t = t.trim();
    if t.is_empty() { "(no title)".to_string() } else { t.to_string() }
}

/// CR and LF to spaces, trimmed: for fields that must stay on one line.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect::<String>().trim().to_string()
}

fn text(s: &str) -> Option<String> {
    let t = s.replace("\r\n", "\n").replace('\r', "\n");
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_string())
}

enum Time {
    Date(NaiveDate),
    At(DateTime<Utc>),
}

fn time(v: Option<&serde_json::Value>) -> Option<Time> {
    let v = v?;
    if let Some(d) = v.get("date").and_then(|x| x.as_str()) {
        return NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().map(Time::Date);
    }
    let at = v.get("dateTime").and_then(|x| x.as_str())?;
    DateTime::parse_from_rfc3339(at).ok().map(|t| Time::At(t.with_timezone(&Utc)))
}

/// An API event, normalised (`None` without an id or a start).
pub fn event(raw: &serde_json::Value) -> Option<Event> {
    let id = raw.get("id").and_then(|v| v.as_str()).filter(|i| !i.is_empty())?;
    let id = id.lines().collect::<Vec<_>>().join(" ").trim().to_string();
    let start = time(raw.get("start"))?;
    let end = time(raw.get("end"));
    let str_field = |k: &str| raw.get(k).and_then(|v| v.as_str());
    let base = Event {
        id,
        summary: summary(str_field("summary").unwrap_or("")),
        location: str_field("location").map(one_line).filter(|l| !l.is_empty()),
        description: str_field("description").and_then(text),
        all_day: false,
        start_date: None,
        end_date_exclusive: None,
        start_utc: None,
        end_utc: None,
        recurring: raw.get("recurrence").is_some_and(|v| !v.is_null()) || str_field("recurringEventId").is_some(),
    };
    Some(match start {
        Time::Date(s) => {
            let e = match end {
                Some(Time::Date(e)) => e,
                _ => s + chrono::Duration::days(1),
            };
            Event { all_day: true, start_date: Some(s), end_date_exclusive: Some(e), ..base }
        }
        Time::At(s) => Event { start_utc: Some(s), end_utc: match end { Some(Time::At(e)) => Some(e), _ => None }, ..base },
    })
}

/// IANA name, `UTC` or `GMT`.
pub fn timezone(name: &str) -> Result<Tz, String> {
    match name {
        "UTC" => Ok(chrono_tz::UTC),
        "GMT" => Ok(chrono_tz::Etc::GMT),
        other => other.parse::<Tz>().map_err(|_| format!("unknown IANA timezone {other:?}")),
    }
}

fn midnight(day: NaiveDate, tz: Tz) -> DateTime<Utc> {
    use chrono::TimeZone;
    let local = day.and_hms_opt(0, 0, 0).expect("midnight exists");
    tz.from_local_datetime(&local).earliest().unwrap_or_else(|| tz.from_utc_datetime(&local)).with_timezone(&Utc)
}

/// [today - past, today + future] in `tz`, as UTC instants (the end exclusive, at midnight).
pub fn window(today: NaiveDate, tz: Tz, past_days: i64, future_days: i64) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    if past_days < 0 || future_days < 0 {
        return Err("past_days and future_days must not be negative".into());
    }
    Ok((midnight(today - chrono::Duration::days(past_days), tz), midnight(today + chrono::Duration::days(future_days + 1), tz)))
}

fn key(e: &Event) -> (u8, i64, String) {
    use chrono::Datelike;
    match (e.start_date, e.start_utc) {
        (Some(d), _) => (0, i64::from(d.num_days_from_ce()), e.id.clone()),
        (_, Some(t)) => (1, t.timestamp(), e.id.clone()),
        _ => (2, 0, e.id.clone()),
    }
}

/// All-day events first by date, then timed ones by start; ties by id.
pub fn sort(events: &mut [Event]) {
    events.sort_by_key(key);
}

fn org_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d %a").to_string()
}

fn org_time(e: &Event, tz: Tz) -> String {
    if let (Some(s), Some(end)) = (e.start_date, e.end_date_exclusive) {
        let last = (end - chrono::Duration::days(1)).max(s);
        return if last == s { format!("<{}>", org_date(s)) } else { format!("<{}>--<{}>", org_date(s), org_date(last)) };
    }
    let s = e.start_utc.expect("timed").with_timezone(&tz);
    match e.end_utc.map(|x| x.with_timezone(&tz)) {
        None => format!("<{} {}>", org_date(s.date_naive()), s.format("%H:%M")),
        Some(x) if x.date_naive() == s.date_naive() => format!("<{} {}-{}>", org_date(s.date_naive()), s.format("%H:%M"), x.format("%H:%M")),
        Some(x) => format!("<{} {}>--<{} {}>", org_date(s.date_naive()), s.format("%H:%M"), org_date(x.date_naive()), x.format("%H:%M")),
    }
}

/// Event text is third-party (anyone can send an invitation): "Local Variables:" in any
/// case would let a block of it set Emacs file-local variables (eval:) when the file opens.
fn no_local_variables(s: &str) -> String {
    const NEEDLE: &str = "local variables:";
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(NEEDLE) {
        let start = at + found;
        out.push_str(&s[at..start + NEEDLE.len() - 1]);
        out.push_str(" -");
        at = start + NEEDLE.len();
    }
    out.push_str(&s[at..]);
    out
}

/// Org's comma escape inside a block: a line that would start a heading or a keyword.
fn org_escape(line: &str) -> String {
    let rest = line.trim_start().trim_start_matches(',');
    if rest.starts_with('*') || rest.starts_with("#+") { format!(",{line}") } else { line.to_string() }
}

/// An Org file: one heading per event with :GCAL_ID:, an Org timestamp in `tz`, location, notes.
pub fn render_org(events: &[Event], tz: Tz) -> String {
    let mut sorted = events.to_vec();
    sort(&mut sorted);
    let mut lines = vec![
        "#+TITLE: Google Calendar (imported)".to_string(),
        "#+FILETAGS: :gcal:".to_string(),
        "# Regenerated by gcal sync; edits here are overwritten.".to_string(),
        String::new(),
    ];
    for e in &sorted {
        lines.push(format!("* {}", no_local_variables(&e.summary)));
        lines.push(":PROPERTIES:".into());
        lines.push(format!(":GCAL_ID: {}", no_local_variables(&e.id)));
        if e.recurring {
            lines.push(":RECURRING: t".into());
        }
        lines.push(":END:".into());
        lines.push(org_time(e, tz));
        if let Some(l) = &e.location {
            lines.push(format!("  Location: {}", no_local_variables(l)));
        }
        if let Some(d) = &e.description {
            lines.push("#+begin_example".into());
            lines.extend(no_local_variables(d).split('\n').map(org_escape));
            lines.push("#+end_example".into());
        }
        lines.push(String::new());
    }
    lines.join("\n").trim_end_matches('\n').to_string() + "\n"
}

fn ical_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

/// RFC 5545 3.1: at most 75 octets per line, continuation lines start with one space; never
/// inside a UTF-8 character.
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in line.chars() {
        let limit = 75;
        let width = c.len_utf8();
        if used + width > limit {
            out.push_str("\r\n ");
            used = 1;
        }
        out.push(c);
        used += width;
    }
    out
}

/// RFC 5545: escaped text, CRLF, lines folded at 75 octets; `dtstamp` for every VEVENT.
pub fn render_ical(events: &[Event], dtstamp: DateTime<Utc>) -> String {
    let mut sorted = events.to_vec();
    sort(&mut sorted);
    let stamp = dtstamp.format("%Y%m%dT%H%M%SZ").to_string();
    let mut lines = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".to_string(), "PRODID:-//rafa//gworkspace gcal//EN".to_string()];
    for e in &sorted {
        lines.push("BEGIN:VEVENT".into());
        lines.push(format!("UID:{}@gsuite.local", ical_text(&e.id)));
        lines.push(format!("DTSTAMP:{stamp}"));
        if let (Some(s), Some(end)) = (e.start_date, e.end_date_exclusive) {
            lines.push(format!("DTSTART;VALUE=DATE:{}", s.format("%Y%m%d")));
            lines.push(format!("DTEND;VALUE=DATE:{}", end.format("%Y%m%d")));
        } else if let Some(s) = e.start_utc {
            lines.push(format!("DTSTART:{}", s.format("%Y%m%dT%H%M%SZ")));
            if let Some(end) = e.end_utc {
                lines.push(format!("DTEND:{}", end.format("%Y%m%dT%H%M%SZ")));
            }
        }
        lines.push(format!("SUMMARY:{}", ical_text(&e.summary)));
        if let Some(l) = &e.location {
            lines.push(format!("LOCATION:{}", ical_text(l)));
        }
        if let Some(d) = &e.description {
            lines.push(format!("DESCRIPTION:{}", ical_text(d)));
        }
        lines.push("END:VEVENT".into());
    }
    lines.push("END:VCALENDAR".into());
    lines.iter().map(|l| fold(l) + "\r\n").collect()
}

pub struct Calendar {
    pub session: Session,
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl Calendar {
    pub async fn events(&self, calendar: &str, from: DateTime<Utc>, until: DateTime<Utc>) -> Result<Vec<Event>, ApiError> {
        let mut url = self.session.url(&["calendars", calendar, "events"]);
        url.query_pairs_mut()
            .append_pair("timeMin", &rfc3339(from))
            .append_pair("timeMax", &rfc3339(until))
            .append_pair("singleEvents", "true")
            .append_pair("orderBy", "startTime")
            .append_pair("maxResults", "250");
        let raw = self.session.paged(&url, "items", None).await?;
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<Event> = raw.iter().filter_map(event).filter(|e| seen.insert(e.id.clone())).collect();
        sort(&mut out);
        Ok(out)
    }

    pub async fn get(&self, calendar: &str, id: &str) -> Result<Event, ApiError> {
        let url = self.session.url(&["calendars", calendar, "events", id]);
        let raw: serde_json::Value = self.session.get(&url).await?;
        event(&raw).ok_or_else(|| ApiError::Failed { api: url.path().to_string(), status: 200, body: "the event has no id or start".into() })
    }
}
