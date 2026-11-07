//! JSON-Schema-to-TOON-header preview synthesizer -- our own addition, with
//! no upstream `toon-format` equivalent. See `specs/toon-format/spec.md`
//! (add-mcp-tool-pipeline change), Requirement: "Generate a TOON-shaped
//! preview from a JSON Schema".

use serde_json::Value;

/// Produces a TOON tabular-header preview string for a JSON Schema
/// describing an object (previewing any array-of-uniform-objects
/// properties) or an array of uniform objects at the root. Returns `None`
/// when the schema describes neither shape (e.g. a bare scalar type).
pub fn schema_preview(schema: &Value) -> Option<String> {
    let obj = schema.as_object()?;
    let ty = obj.get("type").and_then(Value::as_str);

    if ty == Some("array") {
        return array_preview("", obj);
    }

    let properties = obj.get("properties")?.as_object()?;
    if properties.is_empty() {
        return None;
    }
    let lines: Vec<String> = properties
        .iter()
        .map(|(key, prop_schema)| render_property_preview(key, prop_schema))
        .collect();
    Some(lines.join("\n"))
}

fn array_preview(key: &str, arr_obj: &serde_json::Map<String, Value>) -> Option<String> {
    let items_schema = arr_obj.get("items")?;
    let item_obj = items_schema.as_object()?;
    if item_obj.get("type").and_then(Value::as_str) == Some("object") {
        if let Some(props) = item_obj.get("properties").and_then(Value::as_object) {
            let fields: Vec<&str> = props.keys().map(String::as_str).collect();
            return Some(format!("{key}[N]{{{}}}:", fields.join(",")));
        }
    }
    Some(format!("{key}[N]:"))
}

fn render_property_preview(key: &str, schema: &Value) -> String {
    let obj = schema.as_object();
    let ty = obj.and_then(|o| o.get("type")).and_then(Value::as_str);
    if ty == Some("array") {
        if let Some(preview) = obj.and_then(|o| array_preview(key, o)) {
            return preview;
        }
        return format!("{key}[N]:");
    }
    format!("{key}: {}", ty.unwrap_or("value"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn previews_an_array_of_objects_property_in_declared_field_order() {
        let schema = json!({
            "type": "object",
            "properties": {
                "users": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"id": {"type": "number"}, "name": {"type": "string"}}
                    }
                }
            }
        });
        let preview = schema_preview(&schema).unwrap();
        assert_eq!(preview, "users[N]{id,name}:");
    }

    #[test]
    fn previews_a_root_array_of_objects_schema() {
        let schema = json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {"id": {"type": "number"}}
            }
        });
        assert_eq!(schema_preview(&schema).unwrap(), "[N]{id}:");
    }

    #[test]
    fn no_preview_for_a_bare_scalar_schema() {
        let schema = json!({"type": "string"});
        assert_eq!(schema_preview(&schema), None);
    }

    #[test]
    fn no_preview_for_a_schema_with_no_properties() {
        let schema = json!({"type": "object", "properties": {}});
        assert_eq!(schema_preview(&schema), None);
    }
}
