//! Google Sheets API v4: metadata, reading and writing ranges, tabs, CSV export.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ApiError, Session};

pub const BASE: &str = "https://sheets.googleapis.com/v4/spreadsheets";
pub const SHEET_MIME: &str = "application/vnd.google-apps.spreadsheet";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SheetInfo {
    pub sheet_id: i64,
    pub title: String,
    pub index: i64,
    pub row_count: Option<i64>,
    pub column_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpreadsheetMetadata {
    pub spreadsheet_id: String,
    pub title: String,
    pub locale: Option<String>,
    pub url: Option<String>,
    pub sheets: Vec<SheetInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueRange {
    pub range: Option<String>,
    #[serde(default)]
    pub values: Vec<Vec<Value>>,
}

/// A tab title as an A1 range: quoted, `'` doubled, so names like `A1` or `Q1 '24` are tabs (pure).
pub fn tab_range(title: &str) -> String {
    format!("'{}'", title.replace('\'', "''"))
}

fn cell(value: &Value) -> String {
    let text = match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if text.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// RFC 4180 CSV: fields with `,` `"` CR or LF quoted, `"` doubled; numbers/bools as text (pure).
pub fn to_csv(values: &[Vec<Value>]) -> String {
    values.iter().map(|row| row.iter().map(cell).collect::<Vec<_>>().join(",") + "\r\n").collect()
}

pub use crate::file_stem;

fn str_at(v: &Value, pointer: &str) -> Option<String> {
    v.pointer(pointer).and_then(Value::as_str).map(str::to_string)
}

pub struct Sheets {
    pub session: Session,
}

impl Sheets {
    async fn batch(&self, id: &str, requests: Value) -> Result<Value, ApiError> {
        let url = self.session.url(&[&format!("{id}:batchUpdate")]);
        self.session.send(reqwest::Method::POST, &url, &serde_json::json!({ "requests": requests })).await
    }

    pub async fn metadata(&self, id: &str) -> Result<SpreadsheetMetadata, ApiError> {
        let mut url = self.session.url(&[id]);
        url.query_pairs_mut().append_pair("includeGridData", "false");
        let meta: Value = self.session.get(&url).await?;
        let sheets = meta
            .get("sheets")
            .and_then(Value::as_array)
            .map(|tabs| {
                tabs.iter()
                    .filter_map(|s| {
                        let p = s.get("properties")?;
                        Some(SheetInfo {
                            sheet_id: p.get("sheetId")?.as_i64()?,
                            title: p.get("title")?.as_str()?.to_string(),
                            index: p.get("index")?.as_i64()?,
                            row_count: p.pointer("/gridProperties/rowCount").and_then(Value::as_i64),
                            column_count: p.pointer("/gridProperties/columnCount").and_then(Value::as_i64),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(SpreadsheetMetadata {
            spreadsheet_id: str_at(&meta, "/spreadsheetId").unwrap_or_default(),
            title: str_at(&meta, "/properties/title").unwrap_or_default(),
            locale: str_at(&meta, "/properties/locale"),
            url: str_at(&meta, "/spreadsheetUrl"),
            sheets,
        })
    }

    pub async fn read(&self, id: &str, range: &str) -> Result<ValueRange, ApiError> {
        self.session.get(&self.session.url(&[id, "values", range])).await
    }

    pub async fn batch_read(&self, id: &str, ranges: &[String]) -> Result<Vec<ValueRange>, ApiError> {
        let mut url = self.session.url(&[id, "values:batchGet"]);
        for r in ranges {
            url.query_pairs_mut().append_pair("ranges", r);
        }
        #[derive(Deserialize)]
        struct Response {
            #[serde(default, rename = "valueRanges")]
            value_ranges: Vec<ValueRange>,
        }
        let r: Response = self.session.get(&url).await?;
        Ok(r.value_ranges)
    }

    /// `input`: USER_ENTERED (parsed like typing) or RAW.
    pub async fn write(&self, id: &str, range: &str, values: &[Vec<Value>], input: &str) -> Result<Value, ApiError> {
        let mut url = self.session.url(&[id, "values", range]);
        url.query_pairs_mut().append_pair("valueInputOption", input);
        self.session.send(reqwest::Method::PUT, &url, &serde_json::json!({ "values": values })).await
    }

    pub async fn append(&self, id: &str, range: &str, values: &[Vec<Value>]) -> Result<Value, ApiError> {
        let mut url = self.session.url(&[id, "values", &format!("{range}:append")]);
        url.query_pairs_mut().append_pair("valueInputOption", "USER_ENTERED").append_pair("insertDataOption", "INSERT_ROWS");
        self.session.send(reqwest::Method::POST, &url, &serde_json::json!({ "values": values })).await
    }

    pub async fn create(&self, title: &str, tabs: &[String]) -> Result<Value, ApiError> {
        let mut body = serde_json::json!({ "properties": { "title": title } });
        if !tabs.is_empty() {
            body["sheets"] = tabs.iter().map(|t| serde_json::json!({ "properties": { "title": t } })).collect();
        }
        self.session.send(reqwest::Method::POST, &self.session.url(&[]), &body).await
    }

    pub async fn add_tab(&self, id: &str, title: &str) -> Result<Value, ApiError> {
        self.batch(id, serde_json::json!([{ "addSheet": { "properties": { "title": title } } }])).await
    }

    pub async fn delete_tab(&self, id: &str, sheet_id: i64) -> Result<(), ApiError> {
        self.batch(id, serde_json::json!([{ "deleteSheet": { "sheetId": sheet_id } }])).await.map(|_| ())
    }

    /// One (tab title, CSV) per tab.
    pub async fn csv(&self, id: &str) -> Result<Vec<(String, String)>, ApiError> {
        let meta = self.metadata(id).await?;
        self.csv_of(id, &meta).await
    }

    /// [`Sheets::csv`] for metadata already fetched.
    pub async fn csv_of(&self, id: &str, meta: &SpreadsheetMetadata) -> Result<Vec<(String, String)>, ApiError> {
        let mut out = Vec::with_capacity(meta.sheets.len());
        for tab in &meta.sheets {
            let range = self.read(id, &tab_range(&tab.title)).await?;
            out.push((tab.title.clone(), to_csv(&range.values)));
        }
        Ok(out)
    }
}
