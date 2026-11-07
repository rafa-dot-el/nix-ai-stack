use mcp_tokens::{savings, tool_tokens, Savings};
use serde_json::json;

#[test]
fn a_tool_costs_a_stable_nonzero_number_of_tokens() {
    let schema = json!({ "type": "object", "properties": { "query": { "type": "string" } } });
    let a = tool_tokens("search_messages", Some("Search Slack messages."), &schema);
    assert!(a > 10, "{a}");
    assert_eq!(a, tool_tokens("search_messages", Some("Search Slack messages."), &schema));
}

#[test]
fn longer_descriptions_and_schemas_cost_more() {
    let small = json!({ "type": "object" });
    let big = json!({ "type": "object", "properties": {
        "channel": { "type": "string", "description": "Channel id or name" },
        "limit": { "type": "integer", "minimum": 1, "maximum": 1000 } } });
    let base = tool_tokens("t", None, &small);
    assert!(tool_tokens("t", Some("A much longer description of what this tool does."), &small) > base);
    assert!(tool_tokens("t", None, &big) > base);
}

#[test]
fn savings_sum_and_share() {
    assert_eq!(savings(&[100, 300], &[100]), Savings { full: 400, filtered: 100, percent_saved: 75.0 });
    assert_eq!(savings(&[10, 10], &[10, 10]), Savings { full: 20, filtered: 20, percent_saved: 0.0 });
    assert_eq!(savings(&[], &[]), Savings { full: 0, filtered: 0, percent_saved: 0.0 });
}
