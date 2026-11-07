//! Prometheus text exposition. `metrics` is pure; `serve` answers GET <path> on a TCP
//! listener (loopback by default) and 404 elsewhere.
use chrono::{DateTime, Utc};

use crate::model::UsageData;

pub const FETCH_FAILED_BODY: &str = "# HELP claude_fetch_ok Whether the last API fetch succeeded\n\
     # TYPE claude_fetch_ok gauge\n\
     claude_fetch_ok 0\n";

/// A label value escaped per the text format: backslash, double quote, newline (pure).
pub fn label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// The whole exposition for one fetch (pure).
pub fn metrics(usage: &UsageData, now: &DateTime<Utc>) -> String {
    let mut out = String::new();
    out.push_str("# HELP claude_usage_percent Current utilization percentage\n");
    out.push_str("# TYPE claude_usage_percent gauge\n");
    emit_gauge_window(&mut out, "claude_usage_percent", "five_hour", &usage.five_hour);
    emit_gauge_window(&mut out, "claude_usage_percent", "seven_day", &usage.seven_day);
    emit_scoped_limits(&mut out, "claude_usage_percent", usage);
    out.push_str("\n# HELP claude_reset_seconds Seconds until rate limit reset\n");
    out.push_str("# TYPE claude_reset_seconds gauge\n");
    emit_reset_secs(&mut out, "five_hour", &usage.five_hour, now);
    emit_reset_secs(&mut out, "seven_day", &usage.seven_day, now);
    out.push_str("\n# HELP claude_usage_severity Rate limit severity (0=normal, 1=warning, 2=critical)\n");
    out.push_str("# TYPE claude_usage_severity gauge\n");
    emit_severity(&mut out, usage);
    out.push_str("\n# HELP claude_fetch_ok Whether the last API fetch succeeded\n");
    out.push_str("# TYPE claude_fetch_ok gauge\n");
    out.push_str("claude_fetch_ok 1\n");
    out
}

/// Answer scrapes until the listener fails; `scrape` produces the body (or an error, which
/// becomes claude_fetch_ok 0).
pub async fn serve<F, Fut>(listener: tokio::net::TcpListener, path: String, scrape: F) -> std::io::Result<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let scrape = std::sync::Arc::new(scrape);
    let path = std::sync::Arc::new(path);
    loop {
        let (mut sock, _) = listener.accept().await?;
        let (scrape, path) = (scrape.clone(), path.clone());
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let mut n = 0;
            // The request line is enough: GET <path> HTTP/1.x
            while n < buf.len() {
                match tokio::time::timeout(std::time::Duration::from_secs(5), sock.read(&mut buf[n..])).await {
                    Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
                    Ok(Ok(k)) => n += k,
                }
                if buf[..n].windows(2).any(|w| w == b"\r\n") {
                    break;
                }
            }
            let line = String::from_utf8_lossy(&buf[..n]);
            let mut parts = line.split_whitespace();
            let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let (status, body) = if method == "GET" && target == path.as_str() {
                ("200 OK", scrape().await.unwrap_or_else(|e| {
                    eprintln!("[-] claude-usage exporter: {e}");
                    FETCH_FAILED_BODY.to_string()
                }))
            } else {
                ("404 Not Found", "Not Found\n".to_string())
            };
            let reply = format!("HTTP/1.1 {status}\r\ncontent-type: text/plain; version=0.0.4; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            let _ = sock.write_all(reply.as_bytes()).await;
            let _ = sock.shutdown().await;
        });
    }
}

pub(crate) fn emit_gauge_window(
    out: &mut String,
    metric: &str,
    window: &str,
    w: &Option<crate::model::Window>,
) {
    if let Some(ref w) = w {
        if let Some(pct) = w.utilization {
            out.push_str(&format!("{metric}{{window=\"{window}\"}} {pct}\n"));
        }
    }
}

pub(crate) fn emit_scoped_limits(out: &mut String, metric: &str, usage: &UsageData) {
    if let Some(ref limits) = usage.limits {
        for limit in limits {
            if let Some(ref scope) = limit.scope {
                if let Some(ref model) = scope.model {
                    if let Some(ref name) = model.display_name {
                        let key = name.to_lowercase().replace(' ', "_");
                        if let Some(pct) = limit.percent {
                            out.push_str(&format!("{metric}{{window=\"weekly_{}\",model=\"{}\"}} {pct}\n", label(&key), label(name)));
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn emit_reset_secs(
    out: &mut String,
    window: &str,
    w: &Option<crate::model::Window>,
    now: &DateTime<Utc>,
) {
    if let Some(ref w) = w {
        if let Some(ref r) = w.resets_at {
            if let Ok(dt) = DateTime::parse_from_rfc3339(r) {
                let secs = (dt.with_timezone(&Utc) - now).num_seconds().max(0);
                out.push_str(&format!(
                    "claude_reset_seconds{{window=\"{window}\"}} {secs}\n"
                ));
            }
        }
    }
}

pub(crate) fn emit_severity(out: &mut String, usage: &UsageData) {
    if let Some(ref limits) = usage.limits {
        for limit in limits {
            let window = match limit.kind.as_str() {
                "session" => "five_hour",
                "weekly_all" => "seven_day",
                _ => continue,
            };
            let val = severity_to_int(limit.severity.as_deref());
            out.push_str(&format!(
                "claude_usage_severity{{window=\"{window}\"}} {val}\n"
            ));
        }
    }
}

pub(crate) fn severity_to_int(sev: Option<&str>) -> u8 {
    match sev {
        Some("warning") => 1,
        Some("critical") => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn window(util: f64, resets_at: &str) -> Window {
        Window {
            utilization: Some(util),
            resets_at: Some(resets_at.to_string()),
            limit_dollars: None,
            used_dollars: None,
            remaining_dollars: None,
        }
    }

    fn empty_usage() -> UsageData {
        UsageData {
            five_hour: None,
            seven_day: None,
            seven_day_oauth_apps: None,
            seven_day_opus: None,
            seven_day_sonnet: None,
            seven_day_cowork: None,
            seven_day_omelette: None,
            tangelo: None,
            iguana_necktie: None,
            omelette_promotional: None,
            nimbus_quill: None,
            cinder_cove: None,
            amber_ladder: None,
            extra_usage: None,
            limits: None,
            spend: None,
            member_dashboard_available: None,
        }
    }

    // ---- severity_to_int ----

    #[test]
    fn severity_int_normal() {
        assert_eq!(severity_to_int(Some("normal")), 0);
    }

    #[test]
    fn severity_int_warning() {
        assert_eq!(severity_to_int(Some("warning")), 1);
    }

    #[test]
    fn severity_int_critical() {
        assert_eq!(severity_to_int(Some("critical")), 2);
    }

    #[test]
    fn severity_int_none() {
        assert_eq!(severity_to_int(None), 0);
    }

    #[test]
    fn severity_int_unknown() {
        assert_eq!(severity_to_int(Some("unexpected")), 0);
    }

    // ---- emit_gauge_window ----

    #[test]
    fn gauge_window_present() {
        let w = Some(window(36.2, "2026-07-12T20:00:00Z"));
        let mut out = String::new();
        emit_gauge_window(&mut out, "claude_usage_percent", "five_hour", &w);
        assert_eq!(out, "claude_usage_percent{window=\"five_hour\"} 36.2\n");
    }

    #[test]
    fn gauge_window_none() {
        let mut out = String::new();
        emit_gauge_window(&mut out, "claude_usage_percent", "five_hour", &None);
        assert!(out.is_empty());
    }

    #[test]
    fn gauge_window_no_utilization() {
        let w = Some(Window {
            utilization: None,
            resets_at: Some("2026-07-12T20:00:00Z".into()),
            limit_dollars: None,
            used_dollars: None,
            remaining_dollars: None,
        });
        let mut out = String::new();
        emit_gauge_window(&mut out, "m", "w", &w);
        assert!(out.is_empty());
    }

    // ---- emit_scoped_limits ----

    #[test]
    fn scoped_limits_with_models() {
        let mut u = empty_usage();
        u.limits = Some(vec![Limit {
            kind: "weekly_model".into(),
            group: None,
            percent: Some(20.0),
            severity: Some("normal".into()),
            resets_at: None,
            scope: Some(LimitScope {
                model: Some(ScopeModel {
                    id: Some("fable".into()),
                    display_name: Some("Fable".into()),
                }),
                surface: None,
            }),
            is_active: Some(true),
        }]);
        let mut out = String::new();
        emit_scoped_limits(&mut out, "claude_usage_percent", &u);
        assert!(out.contains("weekly_fable"));
        assert!(out.contains("model=\"Fable\""));
        assert!(out.contains("20"));
    }

    #[test]
    fn scoped_limits_no_limits() {
        let u = empty_usage();
        let mut out = String::new();
        emit_scoped_limits(&mut out, "m", &u);
        assert!(out.is_empty());
    }

    #[test]
    fn scoped_limits_no_scope() {
        let mut u = empty_usage();
        u.limits = Some(vec![Limit {
            kind: "session".into(),
            group: None,
            percent: Some(50.0),
            severity: None,
            resets_at: None,
            scope: None,
            is_active: None,
        }]);
        let mut out = String::new();
        emit_scoped_limits(&mut out, "m", &u);
        assert!(out.is_empty());
    }

    // ---- emit_reset_secs ----

    #[test]
    fn reset_secs_future() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let w = Some(window(36.0, "2026-07-12T19:00:00Z"));
        let mut out = String::new();
        emit_reset_secs(&mut out, "five_hour", &w, &now);
        assert!(out.contains("3600"));
    }

    #[test]
    fn reset_secs_past_clamped_to_zero() {
        let now = "2026-07-12T20:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let w = Some(window(36.0, "2026-07-12T18:00:00Z"));
        let mut out = String::new();
        emit_reset_secs(&mut out, "five_hour", &w, &now);
        assert!(out.contains(" 0\n"));
    }

    #[test]
    fn reset_secs_none_window() {
        let now = Utc::now();
        let mut out = String::new();
        emit_reset_secs(&mut out, "five_hour", &None, &now);
        assert!(out.is_empty());
    }

    // ---- emit_severity ----

    #[test]
    fn severity_emits_session_and_weekly() {
        let mut u = empty_usage();
        u.limits = Some(vec![
            Limit {
                kind: "session".into(),
                group: None,
                percent: Some(50.0),
                severity: Some("warning".into()),
                resets_at: None,
                scope: None,
                is_active: None,
            },
            Limit {
                kind: "weekly_all".into(),
                group: None,
                percent: Some(30.0),
                severity: Some("normal".into()),
                resets_at: None,
                scope: None,
                is_active: None,
            },
        ]);
        let mut out = String::new();
        emit_severity(&mut out, &u);
        assert!(out.contains("window=\"five_hour\"} 1"));
        assert!(out.contains("window=\"seven_day\"} 0"));
    }

    #[test]
    fn severity_skips_unknown_kinds() {
        let mut u = empty_usage();
        u.limits = Some(vec![Limit {
            kind: "weekly_model".into(),
            group: None,
            percent: Some(20.0),
            severity: Some("normal".into()),
            resets_at: None,
            scope: None,
            is_active: None,
        }]);
        let mut out = String::new();
        emit_severity(&mut out, &u);
        assert!(out.is_empty());
    }

    #[test]
    fn severity_no_limits() {
        let u = empty_usage();
        let mut out = String::new();
        emit_severity(&mut out, &u);
        assert!(out.is_empty());
    }

    // ---- prometheus output format ----

    #[test]
    fn gauge_line_format() {
        let w = Some(window(42.5, "2026-07-12T20:00:00Z"));
        let mut out = String::new();
        emit_gauge_window(&mut out, "claude_usage_percent", "five_hour", &w);
        // Must match Prometheus text exposition format
        assert!(out.starts_with("claude_usage_percent{"));
        assert!(out.ends_with('\n'));
        assert!(out.contains("window=\"five_hour\""));
    }
}
