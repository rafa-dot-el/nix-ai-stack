mod common;

use serde_json::json;
use tmpmail::api::{id_text, is_read, Client, Session};

#[test]
fn ids_and_read_flags() {
    assert_eq!(id_text(&json!(1)), "1");
    assert_eq!(id_text(&json!("123")), "123", "a string id has no quotes");
    assert_eq!(id_text(&json!(null)), "");
    assert!(is_read(&json!(1)) && is_read(&json!("1")));
    assert!(!is_read(&json!(0)) && !is_read(&json!("0")) && !is_read(&json!(null)));
}

#[tokio::test]
async fn every_call_names_its_action_and_session() {
    let s = common::stub(vec![
        (200, json!({"email_addr": "x@guerrillamailblock.com", "sid_token": "SID"})),
        (200, json!({"list": [{"mail_id": "5", "mail_from": "a@b", "mail_subject": "Hi", "mail_date": "d", "mail_excerpt": "e", "mail_read": "0"}]})),
        (200, json!({"mail_id": 5, "mail_from": "a@b", "mail_subject": "Hi", "mail_date": "d", "mail_body": "<p>x</p>"})),
        (200, json!(true)),
    ])
    .await;
    let c = Client::new(&format!("{}/ajax.php", s.base)).unwrap();
    let sess = c.new_address().await.unwrap();
    assert_eq!(sess, Session { email: "x@guerrillamailblock.com".into(), sid_token: "SID".into() });
    assert_eq!(c.list(&sess, 0).await.unwrap()[0].mail_subject, "Hi");
    assert_eq!(c.fetch(&sess, "5").await.unwrap().mail_body, "<p>x</p>");
    assert!(c.forget(&sess).await.unwrap());
    let seen = s.seen();
    assert!(seen.iter().all(|r| r.method == "GET" && r.target.starts_with("/ajax.php?")));
    assert!(seen[0].target.contains("f=get_email_address") && !seen[0].target.contains("sid_token"));
    assert!(seen[1].target.contains("f=get_email_list") && seen[1].target.contains("offset=0") && seen[1].target.contains("sid_token=SID"));
    assert!(seen[2].target.contains("f=fetch_email") && seen[2].target.contains("email_id=5"));
    assert!(seen[3].target.contains("f=forget_me") && seen[3].target.contains("email_addr=x%40guerrillamailblock.com"));
    assert!(seen[0].head.to_lowercase().contains("user-agent: tmpmail/"), "{}", seen[0].head);
}

#[tokio::test]
async fn http_and_shape_errors_are_reported() {
    let s = common::stub(vec![(503, json!({"e": 1})), (200, json!({"nope": 1})), (200, json!("false"))]).await;
    let c = Client::new(&format!("{}/ajax.php", s.base)).unwrap();
    let sess = Session { email: "x@y".into(), sid_token: "S".into() };
    assert!(c.new_address().await.unwrap_err().contains("503"));
    assert!(c.list(&sess, 0).await.unwrap_err().contains("get_email_list"));
    assert!(!c.forget(&sess).await.unwrap(), "anything but true is not acknowledged");
    assert!(Client::new("http://127.0.0.1:1/ajax.php").unwrap().new_address().await.unwrap_err().contains("get_email_address"));
}
