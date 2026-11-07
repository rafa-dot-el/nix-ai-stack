//! The operations, shared by the CLI and the MCP server. Views never carry a sid_token.
use serde_json::{json, Value};

use crate::{
    api::{id_text, is_read, Client, Session},
    format::{strip_html, IdentityView, MessageDetailView, MessageView},
    identities::{self, Identity},
    setup::Setup,
};

fn view<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("serialisable view")
}

fn session(i: &Identity) -> Session {
    Session { email: i.email.clone(), sid_token: i.sid_token.clone() }
}

pub async fn list_identities(setup: &Setup) -> Result<Value, String> {
    let ids = identities::load(&setup.identities)?;
    Ok(Value::Array(ids.identities.iter().map(|i| view(&IdentityView::from(i))).collect()))
}

pub async fn new_identity(setup: &Setup, now: u64) -> Result<Value, String> {
    let s = Client::new(&setup.endpoint)?.new_address().await?;
    let identity = Identity { email: s.email, sid_token: s.sid_token, created_at: now };
    let mut ids = identities::load(&setup.identities)?;
    ids.upsert(identity.clone());
    identities::save(&setup.identities, &ids)?;
    Ok(view(&IdentityView::from(&identity)))
}

pub async fn list_emails(setup: &Setup, email: Option<&str>) -> Result<Value, String> {
    let ids = identities::load(&setup.identities)?;
    let me = ids.resolve(email)?;
    let list = Client::new(&setup.endpoint)?.list(&session(me), 0).await?;
    Ok(Value::Array(
        list.iter()
            .map(|m| view(&MessageView { id: id_text(&m.mail_id), from: m.mail_from.clone(), subject: m.mail_subject.clone(), date: m.mail_date.clone(), read: is_read(&m.mail_read), excerpt: m.mail_excerpt.clone() }))
            .collect(),
    ))
}

pub async fn get_email(setup: &Setup, id: &str, email: Option<&str>, html: bool) -> Result<Value, String> {
    let ids = identities::load(&setup.identities)?;
    let me = ids.resolve(email)?;
    let m = Client::new(&setup.endpoint)?.fetch(&session(me), id).await?;
    let body = if html { m.mail_body.clone() } else { strip_html(&m.mail_body) };
    Ok(view(&MessageDetailView { id: id_text(&m.mail_id), from: m.mail_from, subject: m.mail_subject, date: m.mail_date, body }))
}

pub async fn forget_identity(setup: &Setup, email: Option<&str>) -> Result<Value, String> {
    let mut ids = identities::load(&setup.identities)?;
    let me = ids.resolve(email)?.clone();
    let acknowledged = Client::new(&setup.endpoint)?.forget(&session(&me)).await?;
    ids.remove(&me.email);
    identities::save(&setup.identities, &ids)?;
    Ok(json!({ "email": me.email, "server_acknowledged": acknowledged }))
}

/// Seconds since the epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
