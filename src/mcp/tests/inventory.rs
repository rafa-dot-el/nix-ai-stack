mod common;

use common::{serve_mock, tool};
use mcp::config::from_value;
use rafa_config::Layer;

#[tokio::test]
async fn inventory_lists_pool_and_unix_servers_raw_and_reports_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let pool = d.path().join("pool");
    std::fs::create_dir(&pool).unwrap();
    let _a = serve_mock(&pool.join("a.sock"), vec![tool("one"), tool("two")]);
    drop(std::os::unix::net::UnixListener::bind(pool.join("dead.sock")).unwrap());
    let remote = d.path().join("remote.sock");
    let _r = serve_mock(&remote, vec![tool("ping")]);
    let yaml = format!(
        "servers:\n  a: {{ exclude: [two] }}\n  remote: {{ transport: {{ unix: \"{}\" }} }}\n  child: {{ transport: {{ stdio: {{ command: x }} }} }}\n",
        remote.display()
    );
    let config = from_value(&Layer::parse("t", &yaml).unwrap().value).unwrap();
    let inv = mcp::app::inventory(&config, &pool).await;
    let names = |k: &str| inv[k].as_ref().unwrap().iter().map(|t| t.name.to_string()).collect::<Vec<_>>();
    assert_eq!(names("a"), vec!["one", "two"], "raw: proxy rules do not apply here");
    assert_eq!(names("remote"), vec!["ping"]);
    assert!(inv["dead"].as_ref().unwrap_err().contains("connect"));
    assert!(inv["child"].as_ref().unwrap_err().contains("stdio"));
}
