//! Encodes `serde_json::Value` into TOON text. See
//! `specs/toon-format/spec.md` (add-mcp-tool-pipeline change), Requirement:
//! "Encode JSON data to TOON".

use serde_json::Value;
use std::collections::BTreeSet;

use crate::error::ToonError;
use crate::options::EncodeOptions;

pub fn encode(value: &Value, opts: &EncodeOptions) -> Result<String, ToonError> {
    let mut out = String::new();
    match value {
        Value::Object(map) => {
            if !map.is_empty() {
                encode_object_body(map, 0, opts, &mut out)?;
            }
        }
        Value::Array(arr) => encode_array(None, arr, 0, opts, &mut out)?,
        other => out.push_str(&format_scalar(other)),
    }
    if out.ends_with('\n') {
        out.pop();
    }
    Ok(out)
}

fn indent_str(level: usize, opts: &EncodeOptions) -> String {
    " ".repeat(level * opts.indent_width)
}

fn encode_object_body(
    map: &serde_json::Map<String, Value>,
    level: usize,
    opts: &EncodeOptions,
    out: &mut String,
) -> Result<(), ToonError> {
    let pad = indent_str(level, opts);
    for (key, value) in map {
        match value {
            Value::Object(inner) if inner.is_empty() => {
                out.push_str(&format!("{pad}{key}: {{}}\n"));
            }
            Value::Object(inner) => {
                out.push_str(&format!("{pad}{key}:\n"));
                encode_object_body(inner, level + 1, opts, out)?;
            }
            Value::Array(arr) => encode_array(Some(key), arr, level, opts, out)?,
            other => out.push_str(&format!("{pad}{key}: {}\n", format_scalar(other))),
        }
    }
    Ok(())
}

fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Object(_) | Value::Array(_))
}

/// Field order eligible for TOON's compact tabular row syntax: a non-empty
/// array of objects sharing the exact same key set, all scalar-valued.
fn tabular_fields(arr: &[Value]) -> Option<Vec<String>> {
    let first = arr.first()?.as_object()?;
    let fields: Vec<String> = first.keys().cloned().collect();
    let field_set: BTreeSet<&str> = fields.iter().map(String::as_str).collect();
    for item in arr {
        let obj = item.as_object()?;
        let obj_set: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        if obj_set != field_set {
            return None;
        }
        if obj.values().any(|v| !is_scalar(v)) {
            return None;
        }
    }
    Some(fields)
}

fn encode_array(
    key: Option<&str>,
    arr: &[Value],
    level: usize,
    opts: &EncodeOptions,
    out: &mut String,
) -> Result<(), ToonError> {
    let pad = indent_str(level, opts);
    let key_prefix = key.unwrap_or("");
    let delim_suffix = opts.delimiter.header_suffix();
    let delim = opts.delimiter.as_char();
    let delim_str = delim.to_string();

    if arr.is_empty() {
        out.push_str(&format!("{pad}{key_prefix}[0{delim_suffix}]:\n"));
        return Ok(());
    }

    if let Some(fields) = tabular_fields(arr) {
        let header_fields = fields.join(&delim_str);
        out.push_str(&format!(
            "{pad}{key_prefix}[{}{delim_suffix}]{{{header_fields}}}:\n",
            arr.len()
        ));
        let row_pad = indent_str(level + 1, opts);
        for item in arr {
            let obj = item.as_object().expect("checked by tabular_fields");
            let cells: Vec<String> = fields
                .iter()
                .map(|f| format_cell(obj.get(f).unwrap_or(&Value::Null), delim))
                .collect();
            out.push_str(&format!("{row_pad}{}\n", cells.join(&delim_str)));
        }
        return Ok(());
    }

    if arr.iter().all(is_scalar) {
        let cells: Vec<String> = arr.iter().map(|v| format_cell(v, delim)).collect();
        out.push_str(&format!(
            "{pad}{key_prefix}[{}{delim_suffix}]: {}\n",
            arr.len(),
            cells.join(&delim_str)
        ));
        return Ok(());
    }

    out.push_str(&format!(
        "{pad}{key_prefix}[{}{delim_suffix}]:\n",
        arr.len()
    ));
    for item in arr {
        encode_list_item(item, level + 1, opts, out)?;
    }
    Ok(())
}

/// Renders one `- ...` list-item line, inlining the item's own first line of
/// content and indenting any continuation lines to align beneath it -- see
/// `design.md`'s decode-side mirror of this convention.
fn encode_list_item(
    item: &Value,
    level: usize,
    opts: &EncodeOptions,
    out: &mut String,
) -> Result<(), ToonError> {
    let pad = indent_str(level, opts);
    match item {
        Value::Object(map) if !map.is_empty() => {
            let mut body = String::new();
            encode_object_body(map, 0, opts, &mut body)?;
            push_item_lines(&pad, &body, out);
        }
        Value::Object(_) => out.push_str(&format!("{pad}- {{}}\n")),
        Value::Array(inner) => {
            let mut body = String::new();
            encode_array(None, inner, 0, opts, &mut body)?;
            push_item_lines(&pad, &body, out);
        }
        other => out.push_str(&format!("{pad}- {}\n", format_scalar(other))),
    }
    Ok(())
}

fn push_item_lines(pad: &str, body: &str, out: &mut String) {
    let mut lines = body.lines();
    let first = lines.next().unwrap_or_default();
    out.push_str(&format!("{pad}- {first}\n"));
    let cont_pad = format!("{pad}  ");
    for line in lines {
        out.push_str(&format!("{cont_pad}{line}\n"));
    }
}

fn format_scalar(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format_string(s, ','),
        _ => unreachable!("format_scalar called on a non-scalar value"),
    }
}

fn format_cell(v: &Value, delim: char) -> String {
    match v {
        Value::String(s) => format_string(s, delim),
        _ => format_scalar(v),
    }
}

/// Quotes a string when leaving it bare would be ambiguous with TOON's own
/// syntax (the active delimiter, a literal quote, a newline, a colon, a
/// leading/trailing-whitespace difference, or a token that would otherwise
/// decode back as `true`/`false`/`null`/a number).
fn format_string(s: &str, delim: char) -> String {
    let needs_quoting = s.is_empty()
        || s.contains(delim)
        || s.contains('"')
        || s.contains(['\n', '\r', '\t'])
        || s.contains(':')
        || s.trim() != s
        || matches!(s, "true" | "false" | "null")
        || s.parse::<f64>().is_ok();
    if needs_quoting {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_a_flat_object() {
        let value = json!({"name": "Alice", "age": 30});
        let toon = encode(&value, &EncodeOptions::default()).unwrap();
        assert_eq!(toon, "name: Alice\nage: 30");
    }

    #[test]
    fn encodes_a_uniform_array_of_objects_as_tabular() {
        let value = json!({"users": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]});
        let toon = encode(&value, &EncodeOptions::default()).unwrap();
        assert_eq!(toon, "users[2]{id,name}:\n  1,Alice\n  2,Bob");
    }

    #[test]
    fn encodes_a_scalar_array_inline() {
        let value = json!({"items": ["a", "b", "c"]});
        let toon = encode(&value, &EncodeOptions::default()).unwrap();
        assert_eq!(toon, "items[3]: a,b,c");
    }

    #[test]
    fn encodes_an_empty_array() {
        let value = json!({"items": []});
        let toon = encode(&value, &EncodeOptions::default()).unwrap();
        assert_eq!(toon, "items[0]:");
    }

    #[test]
    fn encodes_a_root_scalar() {
        let toon = encode(&json!(42), &EncodeOptions::default()).unwrap();
        assert_eq!(toon, "42");
    }

    #[test]
    fn quotes_a_value_containing_the_delimiter() {
        let value = json!({"note": "hello, world"});
        let toon = encode(&value, &EncodeOptions::default()).unwrap();
        assert_eq!(toon, "note: \"hello, world\"");
    }

    #[test]
    fn encodes_a_non_uniform_array_as_list_items() {
        let value = json!({"mixed": [1, {"a": 1, "b": 2}, "text"]});
        let toon = encode(&value, &EncodeOptions::default()).unwrap();
        assert_eq!(toon, "mixed[3]:\n  - 1\n  - a: 1\n    b: 2\n  - text");
    }
}
