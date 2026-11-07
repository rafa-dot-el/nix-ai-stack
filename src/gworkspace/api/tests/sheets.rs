mod common;

use gworkspace_api::{sheets::{file_stem, tab_range, to_csv, Sheets}, Session};
use serde_json::json;

#[test]
fn tab_titles_are_quoted_ranges() {
    assert_eq!(tab_range("Sheet1"), "'Sheet1'");
    assert_eq!(tab_range("A1"), "'A1'", "a tab named like a cell is still the tab");
    assert_eq!(tab_range("Q1 '24"), "'Q1 ''24'");
}

#[test]
fn csv_quotes_only_when_needed() {
    let rows = vec![
        vec![json!("a"), json!("b,c"), json!("say \"hi\"")],
        vec![json!(1), json!(2.5), json!(true), json!(null), json!("x\ny")],
        vec![],
    ];
    assert_eq!(to_csv(&rows), "a,\"b,c\",\"say \"\"hi\"\"\"\r\n1,2.5,true,,\"x\ny\"\r\n\r\n");
    assert_eq!(to_csv(&[]), "");
}

#[test]
fn file_stems_are_safe() {
    assert_eq!(file_stem("Budget 2026", "id"), "Budget 2026");
    assert_eq!(file_stem("../../etc/passwd", "id"), "_.._etc_passwd");
    assert_eq!(file_stem("a\\b\u{7}c", "id"), "a_b_c");
    assert_eq!(file_stem("...", "id"), "id");
    assert_eq!(file_stem("  ", "id"), "id");
}

fn sheets(base: &str) -> Sheets {
    Sheets { session: Session::new("t", format!("{base}/v4/spreadsheets")).unwrap() }
}

fn meta() -> serde_json::Value {
    json!({"spreadsheetId": "S", "properties": {"title": "Book", "locale": "en_GB"}, "spreadsheetUrl": "u",
        "sheets": [{"properties": {"sheetId": 0, "title": "A1", "index": 0, "gridProperties": {"rowCount": 10, "columnCount": 2}}},
                   {"properties": {"sheetId": 7, "title": "Q1 '24", "index": 1}}]})
}

#[tokio::test]
async fn metadata_and_csv_per_tab_with_quoted_ranges() {
    let s = common::stub(vec![
        (200, meta()),
        (200, meta()),
        (200, json!({"range": "'A1'!A1:B1", "values": [["x", "y"]]})),
        (200, json!({"range": "'Q1 ''24'", "values": [[1]]})),
    ])
    .await;
    let c = sheets(&s.base);
    let m = c.metadata("S").await.unwrap();
    assert_eq!((m.title.as_str(), m.locale.as_deref(), m.sheets.len()), ("Book", Some("en_GB"), 2));
    assert_eq!(m.sheets[0].row_count, Some(10));
    assert_eq!(c.csv("S").await.unwrap(), vec![("A1".to_string(), "x,y\r\n".to_string()), ("Q1 '24".to_string(), "1\r\n".to_string())]);
    let seen = s.seen();
    assert_eq!(seen[0].target, "/v4/spreadsheets/S?includeGridData=false");
    assert_eq!(seen[2].target, "/v4/spreadsheets/S/values/'A1'");
    assert_eq!(seen[3].target, "/v4/spreadsheets/S/values/'Q1%20''24'");
}

#[tokio::test]
async fn reads_writes_and_tabs() {
    let s = common::stub(vec![
        (200, json!({"range": "A1:B2", "values": [["1"]]})),
        (200, json!({"valueRanges": [{"range": "A1"}, {"range": "B1", "values": [["2"]]}]})),
        (200, json!({"updatedCells": 2})),
        (200, json!({"updates": {"updatedRows": 1}})),
        (200, json!({"spreadsheetId": "N"})),
        (200, json!({"replies": [{"addSheet": {"properties": {"sheetId": 9, "title": "T"}}}]})),
        (200, json!({})),
    ])
    .await;
    let c = sheets(&s.base);
    assert_eq!(c.read("S", "Sheet1!A1:B2").await.unwrap().values, vec![vec![json!("1")]]);
    assert_eq!(c.batch_read("S", &["A1".into(), "B1".into()]).await.unwrap().len(), 2);
    assert_eq!(c.write("S", "A1", &[vec![json!("=1+1")]], "USER_ENTERED").await.unwrap()["updatedCells"], 2);
    assert_eq!(c.append("S", "Sheet1", &[vec![json!(1)]]).await.unwrap()["updates"]["updatedRows"], 1);
    assert_eq!(c.create("New", &["One".into()]).await.unwrap()["spreadsheetId"], "N");
    assert_eq!(c.add_tab("S", "T").await.unwrap()["replies"][0]["addSheet"]["properties"]["sheetId"], 9);
    c.delete_tab("S", 9).await.unwrap();
    let t: Vec<_> = s.seen().iter().map(|x| format!("{} {}", x.method, x.target)).collect();
    assert_eq!(
        t,
        vec![
            "GET /v4/spreadsheets/S/values/Sheet1!A1:B2",
            "GET /v4/spreadsheets/S/values:batchGet?ranges=A1&ranges=B1",
            "PUT /v4/spreadsheets/S/values/A1?valueInputOption=USER_ENTERED",
            "POST /v4/spreadsheets/S/values/Sheet1:append?valueInputOption=USER_ENTERED&insertDataOption=INSERT_ROWS",
            "POST /v4/spreadsheets",
            "POST /v4/spreadsheets/S:batchUpdate",
            "POST /v4/spreadsheets/S:batchUpdate",
        ]
    );
    let seen = s.seen();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[4].body).unwrap(), json!({"properties": {"title": "New"}, "sheets": [{"properties": {"title": "One"}}]}));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&seen[6].body).unwrap(), json!({"requests": [{"deleteSheet": {"sheetId": 9}}]}));
}

#[tokio::test]
async fn a_spreadsheet_id_is_one_segment_and_never_panics() {
    let s = common::stub(vec![(200, json!({})), (200, json!({}))]).await;
    let c = sheets(&s.base);
    c.delete_tab("../x?y", 1).await.unwrap();
    c.delete_tab("has space", 1).await.unwrap();
    let seen = s.seen();
    assert_eq!(seen[0].target, "/v4/spreadsheets/..%2Fx%3Fy:batchUpdate");
    assert_eq!(seen[1].target, "/v4/spreadsheets/has%20space:batchUpdate");
}
