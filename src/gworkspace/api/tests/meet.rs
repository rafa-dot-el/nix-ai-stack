mod common;

use gworkspace_api::{meet::{meeting, meeting_code, record_name, strip_html, summary, Meet}, Session};
use serde_json::json;

#[test]
fn html_is_stripped_and_entities_decoded() {
    assert_eq!(strip_html("<p>Agenda &amp; notes</p><br>&lt;b&gt; &#65;&#x42; &nbsp;x &bogus;"), "Agenda & notes<b> AB  x &bogus;");
    assert_eq!(strip_html(""), "");
}

#[test]
fn names_and_codes_are_validated() {
    assert_eq!(record_name("conferenceRecords/abc-DEF_1").unwrap(), "conferenceRecords/abc-DEF_1");
    for bad in ["conferenceRecords/../../x", "conferenceRecords/a?b", "spaces/x", "conferenceRecords/", "conferenceRecords/a/transcripts"] {
        assert!(record_name(bad).is_err(), "{bad}");
    }
    assert_eq!(meeting_code("abc-mnop-xyz").unwrap(), "abc-mnop-xyz");
    assert!(meeting_code("abc\" OR 1=1").is_err());
    assert!(meeting_code("").is_err());
}

fn event() -> serde_json::Value {
    json!({"id": "E", "summary": "Sync", "start": {"dateTime": "2026-06-01T09:00:00Z"}, "end": {"dateTime": "2026-06-01T09:45:00Z"},
        "hangoutLink": "https://meet.google.com/abc-mnop-xyz", "conferenceData": {"conferenceId": "abc-mnop-xyz"},
        "description": "<b>Agenda</b>", "organizer": {"email": "o@x.org"},
        "attendees": [{"email": "a@x.org", "responseStatus": "accepted", "organizer": true}, {"email": "b@x.org"}],
        "attachments": [{"title": "Doc", "mimeType": "application/vnd.google-apps.document", "fileId": "F", "fileUrl": "u"}]})
}

#[test]
fn events_become_meetings() {
    let s = summary(&event());
    assert_eq!((s.title.as_str(), s.meet_code.as_deref(), s.attendee_count, s.has_attachments), ("Sync", Some("abc-mnop-xyz"), 2, true));
    assert_eq!(summary(&json!({})).title, "(no title)");
    let m = meeting(&event());
    assert_eq!((m.duration_minutes, m.description.as_str(), m.organizer.as_deref()), (Some(45), "Agenda", Some("o@x.org")));
    assert!(m.attendees[0].organizer && m.attendees[0].response.as_deref() == Some("accepted"));
    assert_eq!(m.attachments[0].file_id.as_deref(), Some("F"));
}

#[tokio::test]
async fn records_participants_entries() {
    let s = common::stub(vec![
        (200, json!({"conferenceRecords": [{"name": "conferenceRecords/R", "startTime": "s"}]})),
        (200, json!({"transcripts": [{"name": "conferenceRecords/R/transcripts/T", "docsDestination": {"document": "D"}}]})),
        (200, json!({"recordings": []})),
        (200, json!({"transcripts": [{"name": "conferenceRecords/R/transcripts/T"}]})),
        (200, json!({"participants": [{"name": "conferenceRecords/R/participants/P", "signedinUser": {"displayName": "Ada"}}]})),
        (200, json!({"transcriptEntries": [{"participant": "conferenceRecords/R/participants/P", "text": "hello", "startTime": "t"}, {"participant": "x/y", "text": "?"}]})),
    ])
    .await;
    let m = Meet { session: Session::new("t", format!("{}/v2", s.base)).unwrap() };
    let records = m.records("abc-mnop-xyz").await.unwrap();
    assert_eq!((records[0].name.as_deref(), records[0].transcripts.len()), (Some("conferenceRecords/R"), 1));
    let entries = m.entries("conferenceRecords/R").await.unwrap();
    assert_eq!(entries[0].speaker, "Ada");
    assert_eq!(entries[1].speaker, "x/y", "unknown participants keep their reference");
    let t: Vec<_> = s.seen().iter().map(|x| x.target.clone()).collect();
    assert_eq!(t[0], "/v2/conferenceRecords?filter=space.meeting_code%3D%22abc-mnop-xyz%22");
    assert_eq!(t[1], "/v2/conferenceRecords/R/transcripts");
    assert_eq!(t[5], "/v2/conferenceRecords/R/transcripts/T/entries");
    assert!(m.participants("conferenceRecords/../x").await.is_err());
    assert!(m.records("x\"y").await.is_err());
    assert_eq!(s.seen().len(), 6, "refused names make no request");
}
