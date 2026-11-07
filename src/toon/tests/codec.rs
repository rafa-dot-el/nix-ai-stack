use serde_json::{json, Value};
use toon::{decode, decode_default, encode, encode_default, schema_preview, DecodeOptions, Delimiter, EncodeOptions, ToonError};

fn round_trip(value: &Value, opts: &EncodeOptions) {
    let text = encode(value, opts).unwrap();
    assert_eq!(&decode(&text, &DecodeOptions::new()).unwrap(), value, "TOON was:\n{text}");
}

#[test]
fn default_wrappers_round_trip_a_nested_document() {
    let value = json!({
        "name": "slack",
        "count": 3,
        "ratio": 0.5,
        "enabled": true,
        "missing": null,
        "tags": ["a", "b"],
        "nested": { "deep": { "x": 1 } },
        "rows": [ { "id": 1, "title": "one" }, { "id": 2, "title": "two" } ]
    });
    let text = encode_default(&value).unwrap();
    assert_eq!(decode_default(&text).unwrap(), value);
}

#[test]
fn every_delimiter_and_indent_round_trips_a_table() {
    let value = json!({ "rows": [ { "a": 1, "b": "x y" }, { "a": 2, "b": "z" } ] });
    for delimiter in [Delimiter::Comma, Delimiter::Tab, Delimiter::Pipe] {
        for indent in [2, 4] {
            round_trip(&value, &EncodeOptions::new().with_delimiter(delimiter).with_indent(indent));
        }
    }
}

#[test]
fn strings_that_look_like_other_types_or_contain_delimiters_survive() {
    let value = json!({ "s": ["true", "42", "", "a,b", "a|b", "line\nbreak", "  padded  ", "- dash", "key: value"] });
    for delimiter in [Delimiter::Comma, Delimiter::Tab, Delimiter::Pipe] {
        round_trip(&value, &EncodeOptions::new().with_delimiter(delimiter));
    }
}

#[test]
fn empty_containers_and_scalars_at_the_top() {
    round_trip(&json!({ "empty_list": [], "empty_obj": {} }), &EncodeOptions::default());
    round_trip(&json!([1, 2, 3]), &EncodeOptions::default());
    round_trip(&json!([]), &EncodeOptions::default());
}

#[test]
fn mixed_lists_round_trip() {
    round_trip(&json!({ "items": [1, "two", { "three": 3 }, [4, 5]] }), &EncodeOptions::default());
    round_trip(&json!({ "objs": [ { "a": 1 }, { "b": { "c": 2 } } ] }), &EncodeOptions::default());
}

#[test]
fn table_length_and_width_mismatches_are_errors() {
    let short = "rows[2]{a,b}:\n  1,2\n";
    assert!(matches!(decode_default(short), Err(ToonError::LengthMismatch { expected: 2, found: 1, .. })));
    let narrow = "rows[1]{a,b}:\n  1\n";
    assert!(matches!(decode_default(narrow), Err(ToonError::FieldCountMismatch { expected: 2, found: 1, .. })));
    let inline = "tags[3]: a,b\n";
    assert!(matches!(decode_default(inline), Err(ToonError::LengthMismatch { expected: 3, found: 2, .. })));
}

#[test]
fn malformed_structure_is_reported_with_its_line() {
    for bad in ["key: value\n    deeper: 1\n", "a: 1\nnot a pair\n", "a: 1\n[1]{x}:\n  1\n"] {
        match decode_default(bad) {
            Err(ToonError::InvalidStructure(msg)) => assert!(msg.contains("line"), "{bad:?}: {msg}"),
            other => panic!("{bad:?} decoded to {other:?}"),
        }
    }
}

#[test]
fn decode_options_builders() {
    assert!(DecodeOptions::default().strict);
    assert!(!DecodeOptions::new().with_strict(false).strict);
    let lax = DecodeOptions::new().with_strict(false);
    assert_eq!(decode("a: 1\n", &lax).unwrap(), json!({ "a": 1 }));
    let opts = EncodeOptions::new().with_delimiter(Delimiter::Pipe).with_indent(3);
    assert_eq!((opts.delimiter, opts.indent_width), (Delimiter::Pipe, 3));
}

#[test]
fn schema_preview_names_the_fields_of_an_object_schema() {
    let schema = json!({ "type": "object", "properties": { "id": { "type": "integer" }, "title": { "type": "string" } } });
    let preview = schema_preview(&schema);
    assert!(format!("{preview:?}").contains("id") && format!("{preview:?}").contains("title"), "{preview:?}");
}
