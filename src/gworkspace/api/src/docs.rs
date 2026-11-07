//! Google Docs API v1: read a document as plain or heading-structured text, create one,
//! append text, delete a range.
use serde::Serialize;
use serde_json::Value;

use crate::{ApiError, Session};

pub const BASE: &str = "https://docs.googleapis.com/v1/documents";

fn runs(paragraph: &Value) -> String {
    paragraph
        .get("elements")
        .and_then(Value::as_array)
        .map(|els| els.iter().filter_map(|e| e.pointer("/textRun/content").and_then(Value::as_str)).collect())
        .unwrap_or_default()
}

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Concatenated text runs of paragraphs, table cells and the table of contents (pure).
pub fn plain_text(elements: &[Value]) -> String {
    let mut text = String::new();
    for el in elements {
        if let Some(p) = el.get("paragraph") {
            text.push_str(&runs(p));
        } else if let Some(table) = el.get("table") {
            for row in array(table, "tableRows") {
                for cell in array(row, "tableCells") {
                    text.push_str(&plain_text(array(cell, "content")));
                }
            }
        } else if let Some(toc) = el.get("tableOfContents") {
            text.push_str(&plain_text(array(toc, "content")));
        }
    }
    text
}

/// Like [`plain_text`], with Markdown heading markers and `a | b` table rows (pure).
pub fn structured_text(elements: &[Value]) -> String {
    let mut out = String::new();
    for el in elements {
        if let Some(p) = el.get("paragraph") {
            let text = runs(p);
            let level = p
                .pointer("/paragraphStyle/namedStyleType")
                .and_then(Value::as_str)
                .and_then(|s| s.strip_prefix("HEADING_"))
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| (1..=6).contains(n));
            match level {
                Some(n) if !text.trim().is_empty() => {
                    out.push_str(&"#".repeat(n));
                    out.push(' ');
                    out.push_str(text.trim_end_matches('\n'));
                    out.push('\n');
                }
                _ => out.push_str(&text),
            }
        } else if let Some(table) = el.get("table") {
            for row in array(table, "tableRows") {
                let cells: Vec<String> = array(row, "tableCells").iter().map(|c| plain_text(array(c, "content")).trim().to_string()).collect();
                out.push_str(&cells.join(" | "));
                out.push('\n');
            }
            out.push('\n');
        } else if let Some(toc) = el.get("tableOfContents") {
            out.push_str(&structured_text(array(toc, "content")));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentText {
    pub document_id: Option<String>,
    pub title: Option<String>,
    pub text: String,
}

fn shape(doc: &Value, text: String) -> DocumentText {
    let field = |k: &str| doc.get(k).and_then(Value::as_str).map(str::to_string);
    DocumentText { document_id: field("documentId"), title: field("title"), text }
}

fn body(doc: &Value) -> &[Value] {
    doc.pointer("/body/content").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

pub struct Docs {
    pub session: Session,
}

impl Docs {
    async fn fetch(&self, id: &str) -> Result<Value, ApiError> {
        self.session.get(&self.session.url(&[id])).await
    }

    async fn batch(&self, id: &str, requests: Value) -> Result<(), ApiError> {
        let url = self.session.url(&[&format!("{id}:batchUpdate")]);
        let _: Value = self.session.send(reqwest::Method::POST, &url, &serde_json::json!({ "requests": requests })).await?;
        Ok(())
    }

    pub async fn get(&self, id: &str, structured: bool) -> Result<DocumentText, ApiError> {
        let doc = self.fetch(id).await?;
        let text = if structured { structured_text(body(&doc)) } else { plain_text(body(&doc)) };
        Ok(shape(&doc, text))
    }

    pub async fn create(&self, title: &str) -> Result<DocumentText, ApiError> {
        let url = self.session.url(&[]);
        let doc: Value = self.session.send(reqwest::Method::POST, &url, &serde_json::json!({ "title": title })).await?;
        Ok(shape(&doc, String::new()))
    }

    /// Append at the end of the body; returns the inserted (start, end) UTF-16 range.
    pub async fn append(&self, id: &str, text: &str) -> Result<(i64, i64), ApiError> {
        let doc = self.fetch(id).await?;
        let end = body(&doc).last().and_then(|e| e.get("endIndex")).and_then(Value::as_i64).unwrap_or(1);
        let at = (end - 1).max(1);
        self.batch(id, serde_json::json!([{ "insertText": { "location": { "index": at }, "text": text } }])).await?;
        Ok((at, at + text.encode_utf16().count() as i64))
    }

    pub async fn delete_range(&self, id: &str, start: i64, end: i64) -> Result<(), ApiError> {
        self.batch(id, serde_json::json!([{ "deleteContentRange": { "range": { "startIndex": start, "endIndex": end } } }])).await
    }
}
