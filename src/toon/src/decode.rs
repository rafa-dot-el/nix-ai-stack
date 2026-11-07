//! Decodes TOON text back into `serde_json::Value`, hand-parsed (no regex
//! dependency) against the line-oriented, indentation-based structure this
//! crate's own encoder produces. See `specs/toon-format/spec.md`
//! (add-mcp-tool-pipeline change), Requirements: "Decode TOON back to JSON",
//! "Reject malformed TOON input".

use serde_json::Value;

use crate::error::ToonError;
use crate::options::DecodeOptions;

#[derive(Debug, Clone)]
struct Line {
    indent: usize,
    content: String,
    number: usize,
}

fn tokenize(input: &str) -> Vec<Line> {
    input
        .lines()
        .enumerate()
        .filter_map(|(idx, raw)| {
            if raw.trim().is_empty() {
                return None;
            }
            let indent = raw.len() - raw.trim_start_matches(' ').len();
            Some(Line {
                indent,
                content: raw.trim_start_matches(' ').trim_end().to_string(),
                number: idx + 1,
            })
        })
        .collect()
}

struct ArrayHeader {
    key: String,
    len: usize,
    delimiter: char,
    fields: Option<Vec<String>>,
    inline_rest: String,
}

/// Matches `<key>[<n><delim?>]{<fields>}:` (tabular) or
/// `<key>[<n><delim?>]:<rest>` (scalar-inline or list-style). `key` may be
/// empty for an anonymous (root or list-item) array.
fn parse_array_header(line: &str) -> Option<ArrayHeader> {
    let bracket_open = line.find('[')?;
    let key = line[..bracket_open].trim().to_string();
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let bracket_close = bracket_open + line[bracket_open..].find(']')?;
    let inside = &line[bracket_open + 1..bracket_close];
    let (num_part, delimiter) = if let Some(stripped) = inside.strip_suffix('|') {
        (stripped, '|')
    } else if let Some(stripped) = inside.strip_suffix('\t') {
        (stripped, '\t')
    } else {
        (inside, ',')
    };
    let len: usize = num_part.trim().parse().ok()?;
    let after_bracket = &line[bracket_close + 1..];

    if let Some(brace_open) = after_bracket.find('{') {
        let brace_close = after_bracket.find('}')?;
        if brace_close < brace_open {
            return None;
        }
        let fields_str = &after_bracket[brace_open + 1..brace_close];
        let fields: Vec<String> = if fields_str.trim().is_empty() {
            vec![]
        } else {
            fields_str.split(delimiter).map(|s| s.trim().to_string()).collect()
        };
        let inline_rest = after_bracket[brace_close + 1..]
            .trim_start()
            .trim_start_matches(':')
            .trim()
            .to_string();
        Some(ArrayHeader {
            key,
            len,
            delimiter,
            fields: Some(fields),
            inline_rest,
        })
    } else {
        let colon = after_bracket.find(':')?;
        let inline_rest = after_bracket[colon + 1..].trim().to_string();
        Some(ArrayHeader {
            key,
            len,
            delimiter,
            fields: None,
            inline_rest,
        })
    }
}

/// Splits one delimited row respecting `"`-quoted cells (with `\"`/`\\`
/// escapes), mirroring `encode::format_string`'s quoting rules.
fn split_row(s: &str, delim: char) -> Vec<String> {
    // Cells keep their quotes and escapes: parse_scalar_token needs them to tell the
    // string "42" from the number 42, and to keep a quoted value's padding.
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if in_quotes {
            current.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            } else if c == '"' {
                in_quotes = false;
            }
        } else if c == '"' {
            in_quotes = true;
            current.push(c);
        } else if c == delim {
            cells.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(c);
        }
    }
    cells.push(current.trim().to_string());
    cells
}

/// The contents of a quoted token: `\\`, `\"`, `\n`, `\r` and `\t` are escapes.
fn unquote(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn parse_scalar_token(tok: &str) -> Value {
    let t = tok.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return Value::String(unquote(&t[1..t.len() - 1]));
    }
    match t {
        "null" => return Value::Null,
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(n) = t.parse::<i64>() {
        return Value::Number(n.into());
    }
    if let Ok(f) = t.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(f) {
            return Value::Number(num);
        }
    }
    Value::String(t.to_string())
}

/// `<ident>:<rest>` at the top level of `content`, or `None` if the prefix
/// before the first colon isn't a bare identifier (e.g. a quoted scalar
/// containing its own colon).
fn looks_like_key_value(content: &str) -> Option<(String, String)> {
    let colon = content.find(':')?;
    let key = content[..colon].trim();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some((key.to_string(), content[colon + 1..].trim().to_string()))
}

fn collect_children(lines: &[Line], start: usize, parent_indent: usize) -> (Vec<Line>, usize) {
    let mut children = Vec::new();
    let mut i = start;
    while i < lines.len() && lines[i].indent > parent_indent {
        children.push(lines[i].clone());
        i += 1;
    }
    let consumed = i - start;
    (children, consumed)
}

fn build_array_value(header: &ArrayHeader, children: &[Line]) -> Result<Value, ToonError> {
    if let Some(fields) = &header.fields {
        if children.len() != header.len {
            return Err(ToonError::LengthMismatch {
                key: header.key.clone(),
                expected: header.len,
                found: children.len(),
            });
        }
        let mut out = Vec::with_capacity(children.len());
        for line in children {
            let cells = split_row(&line.content, header.delimiter);
            if cells.len() != fields.len() {
                return Err(ToonError::FieldCountMismatch {
                    key: header.key.clone(),
                    expected: fields.len(),
                    found: cells.len(),
                });
            }
            let mut obj = serde_json::Map::new();
            for (f, c) in fields.iter().zip(cells.iter()) {
                obj.insert(f.clone(), parse_scalar_token(c));
            }
            out.push(Value::Object(obj));
        }
        return Ok(Value::Array(out));
    }

    if !header.inline_rest.is_empty() {
        let cells = split_row(&header.inline_rest, header.delimiter);
        if cells.len() != header.len {
            return Err(ToonError::LengthMismatch {
                key: header.key.clone(),
                expected: header.len,
                found: cells.len(),
            });
        }
        return Ok(Value::Array(cells.iter().map(|c| parse_scalar_token(c)).collect()));
    }

    if header.len == 0 {
        return Ok(Value::Array(vec![]));
    }

    let items = split_list_items(children)?;
    if items.len() != header.len {
        return Err(ToonError::LengthMismatch {
            key: header.key.clone(),
            expected: header.len,
            found: items.len(),
        });
    }
    Ok(Value::Array(items))
}

fn split_list_items(children: &[Line]) -> Result<Vec<Value>, ToonError> {
    if children.is_empty() {
        return Ok(vec![]);
    }
    let item_indent = children[0].indent;
    let mut items = Vec::new();
    let mut i = 0;
    while i < children.len() {
        let line = &children[i];
        if line.indent != item_indent || !line.content.starts_with('-') {
            return Err(ToonError::InvalidStructure(format!(
                "line {}: expected a list item (`- ...`)",
                line.number
            )));
        }
        let rest = line.content[1..].trim_start().to_string();
        let rest_line_no = line.number;
        let mut j = i + 1;
        let mut continuations = Vec::new();
        while j < children.len() && children[j].indent > item_indent {
            continuations.push(children[j].clone());
            j += 1;
        }
        items.push(parse_item_block(&rest, rest_line_no, item_indent, &continuations)?);
        i = j;
    }
    Ok(items)
}

fn parse_item_block(rest: &str, rest_line_no: usize, item_indent: usize, continuations: &[Line]) -> Result<Value, ToonError> {
    let trimmed = rest.trim();
    if let Some(header) = parse_array_header(trimmed) {
        return build_array_value(&header, continuations);
    }
    if trimmed.is_empty() || trimmed == "{}" {
        return Ok(Value::Object(Default::default()));
    }
    if continuations.is_empty() {
        if let Some((key, val)) = looks_like_key_value(trimmed) {
            let mut map = serde_json::Map::new();
            let value = if val.is_empty() || val == "{}" {
                Value::Object(Default::default())
            } else {
                parse_scalar_token(&val)
            };
            map.insert(key, value);
            return Ok(Value::Object(map));
        }
        return Ok(parse_scalar_token(trimmed));
    }
    // The encoder writes an item's later lines two columns in from its `- `, so the text
    // after `- ` sits at that level too; a deeper first continuation is the first line's
    // child (`- b:` / `c: 2`), not its sibling.
    let indent = item_indent + 2;
    let mut all = vec![Line {
        indent,
        content: trimmed.to_string(),
        number: rest_line_no,
    }];
    all.extend_from_slice(continuations);
    Ok(Value::Object(parse_object_lines(&all, indent)?))
}

fn parse_object_lines(lines: &[Line], indent: usize) -> Result<serde_json::Map<String, Value>, ToonError> {
    let mut map = serde_json::Map::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if line.indent != indent {
            return Err(ToonError::InvalidStructure(format!(
                "line {}: unexpected indentation",
                line.number
            )));
        }
        let content = line.content.as_str();
        if content.starts_with('-') {
            return Err(ToonError::InvalidStructure(format!(
                "line {}: unexpected list item outside of an array",
                line.number
            )));
        }
        if let Some(header) = parse_array_header(content) {
            if header.key.is_empty() {
                return Err(ToonError::InvalidStructure(format!(
                    "line {}: array header missing a field name",
                    line.number
                )));
            }
            let (children, consumed) = collect_children(lines, i + 1, indent);
            let value = build_array_value(&header, &children)?;
            map.insert(header.key.clone(), value);
            i += 1 + consumed;
            continue;
        }
        let (key, rest) = looks_like_key_value(content).ok_or_else(|| {
            ToonError::InvalidStructure(format!(
                "line {}: expected `key: value` or `key:`",
                line.number
            ))
        })?;
        if rest.is_empty() {
            let (children, consumed) = collect_children(lines, i + 1, indent);
            if children.is_empty() {
                map.insert(key, Value::Object(Default::default()));
            } else {
                let child_indent = children[0].indent;
                map.insert(key, Value::Object(parse_object_lines(&children, child_indent)?));
            }
            i += 1 + consumed;
        } else if rest == "{}" {
            map.insert(key, Value::Object(Default::default()));
            i += 1;
        } else {
            map.insert(key, parse_scalar_token(&rest));
            i += 1;
        }
    }
    Ok(map)
}

pub fn decode(input: &str, _opts: &DecodeOptions) -> Result<Value, ToonError> {
    let lines = tokenize(input);
    if lines.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    let base_indent = lines[0].indent;

    if lines[0].content.starts_with('-') {
        return Ok(Value::Array(split_list_items(&lines)?));
    }
    if let Some(header) = parse_array_header(&lines[0].content) {
        if header.key.is_empty() {
            let (children, _consumed) = collect_children(&lines, 1, base_indent);
            return build_array_value(&header, &children);
        }
        // Non-empty key: a keyed array header, handled generically below --
        // must not fall into the single-line "root scalar" check next.
        return Ok(Value::Object(parse_object_lines(&lines, base_indent)?));
    }
    if lines.len() == 1 && looks_like_key_value(&lines[0].content).is_none() {
        return Ok(parse_scalar_token(&lines[0].content));
    }
    Ok(Value::Object(parse_object_lines(&lines, base_indent)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::encode;
    use crate::options::EncodeOptions;
    use serde_json::json;

    fn rt(value: Value) {
        let toon = encode(&value, &EncodeOptions::default()).unwrap();
        let decoded = decode(&toon, &DecodeOptions::default()).unwrap();
        assert_eq!(decoded, value, "round-trip mismatch via:\n{toon}");
    }

    #[test]
    fn decodes_a_flat_object() {
        let value = decode("name: Alice\nage: 30", &DecodeOptions::default()).unwrap();
        assert_eq!(value, json!({"name": "Alice", "age": 30}));
    }

    #[test]
    fn decodes_a_tabular_array() {
        let value = decode(
            "users[2]{id,name}:\n  1,Alice\n  2,Bob",
            &DecodeOptions::default(),
        )
        .unwrap();
        assert_eq!(
            value,
            json!({"users": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]})
        );
    }

    #[test]
    fn rejects_length_mismatch() {
        let err = decode("users[2]{id,name}:\n  1,Alice", &DecodeOptions::default()).unwrap_err();
        assert!(matches!(err, ToonError::LengthMismatch { expected: 2, found: 1, .. }));
    }

    #[test]
    fn round_trips_flat_object() {
        rt(json!({"name": "Alice", "age": 30, "active": true}));
    }

    #[test]
    fn round_trips_tabular_array() {
        rt(json!({"users": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]}));
    }

    #[test]
    fn round_trips_scalar_array() {
        rt(json!({"items": ["a", "b", "c"]}));
    }

    #[test]
    fn round_trips_empty_array() {
        rt(json!({"items": []}));
    }

    #[test]
    fn round_trips_nested_object() {
        rt(json!({"data": {"meta": {"count": 3}}}));
    }

    #[test]
    fn round_trips_non_uniform_array() {
        rt(json!({"mixed": [1, {"a": 1, "b": 2}, "text", true, null]}));
    }

    #[test]
    fn round_trips_values_needing_quoting() {
        rt(json!({"note": "hello, world", "time": "12:30", "flag": "true", "n": "42"}));
    }

    #[test]
    fn round_trips_root_scalar() {
        rt(json!(42));
        rt(json!("hello"));
        rt(json!(true));
    }

    #[test]
    fn round_trips_root_array() {
        rt(json!([{"id": 1}, {"id": 2}]));
        rt(json!(["a", "b"]));
    }

    #[test]
    fn round_trips_array_of_arrays() {
        rt(json!({"grid": [[1, 2], [3, 4]]}));
    }
}
