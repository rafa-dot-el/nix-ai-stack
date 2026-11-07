use gworkspace::{config::from_value, output::{render, Format}};
use rafa_config::Layer;

#[test]
fn no_file_means_not_configured_and_default_endpoints() {
    let c = from_value(None).unwrap();
    assert!(c.auth.is_none());
    assert_eq!(c.endpoint("tasks", "https://tasks.googleapis.com/tasks/v1"), "https://tasks.googleapis.com/tasks/v1");
}

#[test]
fn auth_and_endpoints() {
    let l = Layer::parse("g", "version: 1\nauth:\n  client: { file: /c }\n  refresh_token: { file: /r }\nendpoints: { tasks: \"http://127.0.0.1:9/t\" }\n").unwrap();
    let c = from_value(Some(&l.value)).unwrap();
    assert!(c.auth.is_some());
    assert_eq!(c.endpoint("tasks", "x"), "http://127.0.0.1:9/t");
    assert_eq!(c.endpoint("token", "dflt"), "dflt");
}

#[test]
fn a_broken_auth_or_endpoints_section_is_an_error() {
    let l = Layer::parse("g", "auth:\n  client: inline\n  refresh_token: { file: /r }\n").unwrap();
    assert!(from_value(Some(&l.value)).unwrap_err().to_string().contains("auth.client"));
    let l = Layer::parse("g", "endpoints: { tasks: 3 }\n").unwrap();
    assert!(from_value(Some(&l.value)).unwrap_err().to_string().contains("endpoints.tasks"));
}

#[test]
fn render_formats() {
    let v = serde_json::json!({"a": [1, 2]});
    assert_eq!(render(&v, Format::default(), |_| "human".into()), "human");
    assert_eq!(render(&v, Format { json: true, yaml: false }, |_| unreachable!()), "{\n  \"a\": [\n    1,\n    2\n  ]\n}");
    assert_eq!(render(&v, Format { json: false, yaml: true }, |_| unreachable!()), "a:\n- 1\n- 2\n");
}
