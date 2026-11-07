//! A recording HTTP stub: answers requests from a queue of (status, JSON body) in order and
//! records (method, path?query, body, authorization) for each.
#![allow(dead_code)]
use std::sync::{Arc, Mutex};

use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

#[derive(Debug, Clone)]
pub struct Seen {
    /// The request line and headers.
    pub head: String,
    pub method: String,
    pub target: String,
    pub body: String,
    pub auth: String,
}

pub struct Stub {
    pub base: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

pub async fn stub(replies: Vec<(u16, serde_json::Value)>) -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (s, replies) = (seen.clone(), Arc::new(Mutex::new(replies.into_iter())));
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = sock.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let len = text[..head_end]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if buf.len() >= head_end + 4 + len || n == 0 {
                        break;
                    }
                }
                if n == 0 { break }
            }
            let text = String::from_utf8_lossy(&buf).to_string();
            let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
            let mut first = head.lines().next().unwrap_or("").split(' ');
            let auth = head.lines().find_map(|l| l.strip_prefix("authorization: ").or(l.strip_prefix("Authorization: "))).unwrap_or("").to_string();
            s.lock().unwrap().push(Seen { head: head.to_string(), method: first.next().unwrap_or("").into(), target: first.next().unwrap_or("").into(), body: body.into(), auth });
            let (status, reply) = replies.lock().unwrap().next().unwrap_or((500, serde_json::json!({"error": "stub ran out of replies"})));
            let reply = reply.to_string();
            let out = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len());
            sock.write_all(out.as_bytes()).await.unwrap();
        }
    });
    Stub { base, seen }
}

impl Stub {
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}
