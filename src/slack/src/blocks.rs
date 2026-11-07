//! Block Kit for a message (plan 5.1): markdown blocks within Slack's limits, CSV tables as
//! `table` blocks, split when they exceed a message (pure).
use serde_json::Value;

/// Slack: a `markdown` block's text is at most 12,000 characters.
pub const MARKDOWN_MAX: usize = 12_000;
/// Slack: a `table` block holds at most 100 rows and 20 columns.
pub const TABLE_MAX_ROWS: usize = 100;
pub const TABLE_MAX_COLUMNS: usize = 20;

/// Markdown split into pieces of at most `MARKDOWN_MAX` characters, at line breaks when
/// possible; one `markdown` block per piece (one message each).
pub fn markdown_messages(markdown: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut rest = markdown;
    while rest.chars().count() > MARKDOWN_MAX {
        let limit = rest.char_indices().nth(MARKDOWN_MAX).map_or(rest.len(), |(i, _)| i);
        let cut = rest[..limit].rfind('\n').map(|i| i + 1).filter(|&i| i > 0).unwrap_or(limit);
        out.push(serde_json::json!({ "type": "markdown", "text": &rest[..cut] }));
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        out.push(serde_json::json!({ "type": "markdown", "text": rest }));
    }
    out
}

/// RFC 4180 CSV (quoted fields, "" escapes, CRLF or LF) into rows (pure).
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            other => field.push(other),
        }
    }
    if quoted {
        return Err("CSV: a quoted field is not closed".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

/// `table` blocks of at most `rows_per_message` rows each, the header row repeated on every
/// part; more than 20 columns is an error.
pub fn table_messages(rows: &[Vec<String>], rows_per_message: usize) -> Result<Vec<Value>, String> {
    let Some((header, data)) = rows.split_first() else { return Ok(Vec::new()) };
    if let Some(wide) = rows.iter().find(|r| r.len() > TABLE_MAX_COLUMNS) {
        return Err(format!("a table row has {} columns; Slack allows at most {TABLE_MAX_COLUMNS}", wide.len()));
    }
    let cells = |r: &Vec<String>| Value::Array(r.iter().map(|c| serde_json::json!({ "type": "raw_text", "text": c })).collect());
    let per = rows_per_message.clamp(2, TABLE_MAX_ROWS) - 1;
    let chunks: Vec<&[Vec<String>]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(per).collect() };
    Ok(chunks
        .into_iter()
        .map(|chunk| {
            let all: Vec<Value> = std::iter::once(header).chain(chunk).map(cells).collect();
            serde_json::json!({ "type": "table", "rows": all })
        })
        .collect())
}
