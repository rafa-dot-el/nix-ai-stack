use crate::{creds, model::UsageData};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub fn build(usage: &UsageData) -> HashMap<String, String> {
    let now = Utc::now();
    let mut vars = HashMap::new();

    if let Some(ref w) = usage.five_hour {
        vars.insert("PCT_5H".into(), fmt_pct(w.utilization));
        if let Some(ref r) = w.resets_at {
            vars.insert("RESET_5H".into(), fmt_time_local(r));
            vars.insert("COUNTDOWN_5H".into(), fmt_countdown(r, &now));
        }
    }

    if let Some(ref w) = usage.seven_day {
        vars.insert("PCT_WEEK".into(), fmt_pct(w.utilization));
        if let Some(ref r) = w.resets_at {
            vars.insert("RESET_WEEK".into(), fmt_date(r));
            vars.insert("COUNTDOWN_WEEK".into(), fmt_countdown(r, &now));
        }
    }

    if let Some(ref limits) = usage.limits {
        for limit in limits {
            vars.insert(
                format!("SEV_{}", severity_key(&limit.kind)),
                limit.severity.clone().unwrap_or_else(|| "unknown".into()),
            );

            if let Some(ref scope) = limit.scope {
                if let Some(ref model) = scope.model {
                    if let Some(ref name) = model.display_name {
                        let key = name.to_uppercase().replace(' ', "_");
                        vars.insert(format!("PCT_{key}"), fmt_pct(limit.percent));
                        if let Some(ref r) = limit.resets_at {
                            vars.insert(format!("RESET_{key}"), fmt_date(r));
                            vars.insert(format!("COUNTDOWN_{key}"), fmt_countdown(r, &now));
                        }
                        vars.insert(
                            format!("SEV_{key}"),
                            limit.severity.clone().unwrap_or_else(|| "unknown".into()),
                        );
                    }
                }
            }
        }
    }

    if !vars.contains_key("SEV_5H") {
        if let Some(ref limits) = usage.limits {
            for l in limits {
                if l.kind == "session" {
                    vars.insert(
                        "SEV_5H".into(),
                        l.severity.clone().unwrap_or_else(|| "unknown".into()),
                    );
                }
            }
        }
    }
    if !vars.contains_key("SEV_WEEK") {
        if let Some(ref limits) = usage.limits {
            for l in limits {
                if l.kind == "weekly_all" {
                    vars.insert(
                        "SEV_WEEK".into(),
                        l.severity.clone().unwrap_or_else(|| "unknown".into()),
                    );
                }
            }
        }
    }

    vars.insert(
        "COUNTDOWN_NEXT_RESET".into(),
        next_reset_countdown(usage, &now),
    );

    vars
}

pub fn add_creds_vars(vars: &mut HashMap<String, String>, credentials_path: &str) {
    if let Some(plan) = creds::subscription_type(credentials_path) {
        vars.insert("PLAN".into(), plan);
    }
    if let Some(tier) = creds::rate_limit_tier(credentials_path) {
        vars.insert("TIER".into(), tier);
    }
}

pub(crate) fn severity_key(kind: &str) -> &str {
    match kind {
        "session" => "5H",
        "weekly_all" => "WEEK",
        _ => kind,
    }
}

fn fmt_pct(val: Option<f64>) -> String {
    match val {
        Some(v) => format!("{}", v.round() as i64),
        None => "-".into(),
    }
}

fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn fmt_time_local(iso: &str) -> String {
    match parse_iso(iso) {
        Some(dt) => {
            let local = dt.with_timezone(&chrono::Local);
            local.format("%H:%M").to_string()
        }
        None => "-".into(),
    }
}

fn fmt_date(iso: &str) -> String {
    match parse_iso(iso) {
        Some(dt) => {
            let local = dt.with_timezone(&chrono::Local);
            local.format("%b %d").to_string()
        }
        None => "-".into(),
    }
}

fn fmt_countdown(iso: &str, now: &DateTime<Utc>) -> String {
    match parse_iso(iso) {
        Some(reset) => {
            let delta = reset.signed_duration_since(*now);
            if delta.num_seconds() <= 0 {
                return "now".into();
            }
            let total_mins = delta.num_minutes();
            let days = total_mins / (24 * 60);
            let hours = (total_mins % (24 * 60)) / 60;
            let mins = total_mins % 60;
            if days > 0 {
                format!("{days}d {hours}h")
            } else if hours > 0 {
                format!("{hours}h {mins}m")
            } else {
                format!("{mins}m")
            }
        }
        None => "-".into(),
    }
}

pub(crate) fn next_reset_countdown(usage: &UsageData, now: &DateTime<Utc>) -> String {
    let mut earliest: Option<DateTime<Utc>> = None;

    if let Some(ref w) = usage.five_hour {
        if let Some(ref r) = w.resets_at {
            if let Some(dt) = parse_iso(r) {
                if dt > *now && (earliest.is_none() || dt < earliest.unwrap()) {
                    earliest = Some(dt);
                }
            }
        }
    }
    if let Some(ref w) = usage.seven_day {
        if let Some(ref r) = w.resets_at {
            if let Some(dt) = parse_iso(r) {
                if dt > *now && (earliest.is_none() || dt < earliest.unwrap()) {
                    earliest = Some(dt);
                }
            }
        }
    }

    match earliest {
        Some(reset) => {
            let iso = reset.to_rfc3339();
            fmt_countdown(&iso, now)
        }
        None => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

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

    fn window(util: f64, resets_at: &str) -> Window {
        Window {
            utilization: Some(util),
            resets_at: Some(resets_at.to_string()),
            limit_dollars: None,
            used_dollars: None,
            remaining_dollars: None,
        }
    }

    fn limit(kind: &str, pct: f64, severity: &str, resets_at: &str) -> Limit {
        Limit {
            kind: kind.to_string(),
            group: None,
            percent: Some(pct),
            severity: Some(severity.to_string()),
            resets_at: Some(resets_at.to_string()),
            scope: None,
            is_active: Some(true),
        }
    }

    fn scoped_limit(model_name: &str, pct: f64, severity: &str, resets_at: &str) -> Limit {
        Limit {
            kind: "weekly_model".to_string(),
            group: None,
            percent: Some(pct),
            severity: Some(severity.to_string()),
            resets_at: Some(resets_at.to_string()),
            scope: Some(LimitScope {
                model: Some(ScopeModel {
                    id: Some(model_name.to_lowercase()),
                    display_name: Some(model_name.to_string()),
                }),
                surface: None,
            }),
            is_active: Some(true),
        }
    }

    // ---- build() scenarios ----

    #[test]
    fn build_empty_usage() {
        let vars = build(&empty_usage());
        assert_eq!(vars.get("COUNTDOWN_NEXT_RESET").unwrap(), "-");
        assert!(!vars.contains_key("PCT_5H"));
        assert!(!vars.contains_key("PCT_WEEK"));
    }

    #[test]
    fn build_five_hour_only() {
        let mut u = empty_usage();
        u.five_hour = Some(window(42.0, "2026-07-12T20:00:00Z"));
        let vars = build(&u);
        assert_eq!(vars.get("PCT_5H").unwrap(), "42");
        assert!(vars.contains_key("RESET_5H"));
        assert!(vars.contains_key("COUNTDOWN_5H"));
        assert!(!vars.contains_key("PCT_WEEK"));
    }

    #[test]
    fn build_seven_day_only() {
        let mut u = empty_usage();
        u.seven_day = Some(window(15.0, "2026-07-17T00:00:00Z"));
        let vars = build(&u);
        assert_eq!(vars.get("PCT_WEEK").unwrap(), "15");
        assert!(vars.contains_key("RESET_WEEK"));
        assert!(!vars.contains_key("PCT_5H"));
    }

    #[test]
    fn build_both_windows() {
        let mut u = empty_usage();
        u.five_hour = Some(window(36.0, "2026-07-12T20:00:00Z"));
        u.seven_day = Some(window(11.0, "2026-07-17T00:00:00Z"));
        let vars = build(&u);
        assert_eq!(vars.get("PCT_5H").unwrap(), "36");
        assert_eq!(vars.get("PCT_WEEK").unwrap(), "11");
    }

    #[test]
    fn build_severity_from_limits() {
        let mut u = empty_usage();
        u.limits = Some(vec![
            limit("session", 78.0, "warning", "2026-07-12T22:00:00Z"),
            limit("weekly_all", 65.0, "normal", "2026-07-17T00:00:00Z"),
        ]);
        let vars = build(&u);
        assert_eq!(vars.get("SEV_5H").unwrap(), "warning");
        assert_eq!(vars.get("SEV_WEEK").unwrap(), "normal");
    }

    #[test]
    fn build_severity_critical() {
        let mut u = empty_usage();
        u.limits = Some(vec![limit(
            "session",
            98.0,
            "critical",
            "2026-07-12T19:00:00Z",
        )]);
        let vars = build(&u);
        assert_eq!(vars.get("SEV_5H").unwrap(), "critical");
    }

    #[test]
    fn build_scoped_model_vars() {
        let mut u = empty_usage();
        u.limits = Some(vec![scoped_limit(
            "Fable",
            20.0,
            "normal",
            "2026-07-17T00:00:00Z",
        )]);
        let vars = build(&u);
        assert_eq!(vars.get("PCT_FABLE").unwrap(), "20");
        assert_eq!(vars.get("SEV_FABLE").unwrap(), "normal");
        assert!(vars.contains_key("RESET_FABLE"));
        assert!(vars.contains_key("COUNTDOWN_FABLE"));
    }

    #[test]
    fn build_multi_model_vars() {
        let mut u = empty_usage();
        u.limits = Some(vec![
            scoped_limit("Fable", 20.0, "normal", "2026-07-17T00:00:00Z"),
            scoped_limit("Opus 3", 45.0, "warning", "2026-07-17T00:00:00Z"),
        ]);
        let vars = build(&u);
        assert_eq!(vars.get("PCT_FABLE").unwrap(), "20");
        assert_eq!(vars.get("PCT_OPUS_3").unwrap(), "45");
        assert_eq!(vars.get("SEV_OPUS_3").unwrap(), "warning");
    }

    #[test]
    fn build_utilization_zero() {
        let mut u = empty_usage();
        u.five_hour = Some(window(0.0, "2026-07-12T20:00:00Z"));
        let vars = build(&u);
        assert_eq!(vars.get("PCT_5H").unwrap(), "0");
    }

    #[test]
    fn build_utilization_100() {
        let mut u = empty_usage();
        u.five_hour = Some(window(100.0, "2026-07-12T20:00:00Z"));
        let vars = build(&u);
        assert_eq!(vars.get("PCT_5H").unwrap(), "100");
    }

    #[test]
    fn build_utilization_fractional_rounds() {
        let mut u = empty_usage();
        u.five_hour = Some(window(33.7, "2026-07-12T20:00:00Z"));
        let vars = build(&u);
        assert_eq!(vars.get("PCT_5H").unwrap(), "34");
    }

    #[test]
    fn build_utilization_none() {
        let mut u = empty_usage();
        u.five_hour = Some(Window {
            utilization: None,
            resets_at: Some("2026-07-12T20:00:00Z".into()),
            limit_dollars: None,
            used_dollars: None,
            remaining_dollars: None,
        });
        let vars = build(&u);
        assert_eq!(vars.get("PCT_5H").unwrap(), "-");
    }

    // ---- fmt_pct ----

    #[test]
    fn test_fmt_pct_some() {
        assert_eq!(fmt_pct(Some(36.2)), "36");
        assert_eq!(fmt_pct(Some(0.0)), "0");
        assert_eq!(fmt_pct(Some(100.0)), "100");
        assert_eq!(fmt_pct(Some(99.9)), "100");
        assert_eq!(fmt_pct(Some(99.4)), "99");
    }

    #[test]
    fn test_fmt_pct_none() {
        assert_eq!(fmt_pct(None), "-");
    }

    // ---- fmt_countdown ----

    #[test]
    fn countdown_minutes_only() {
        let now = "2026-07-12T18:30:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-12T18:55:00Z", &now), "25m");
    }

    #[test]
    fn countdown_hours_and_minutes() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-12T20:30:00Z", &now), "2h 30m");
    }

    #[test]
    fn countdown_days_and_hours() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-17T00:00:00Z", &now), "4d 6h");
    }

    #[test]
    fn countdown_past_reset() {
        let now = "2026-07-12T20:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-12T18:00:00Z", &now), "now");
    }

    #[test]
    fn countdown_exactly_now() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-12T18:00:00Z", &now), "now");
    }

    #[test]
    fn countdown_invalid_iso() {
        let now = Utc::now();
        assert_eq!(fmt_countdown("not-a-date", &now), "-");
    }

    #[test]
    fn countdown_one_minute() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-12T18:01:00Z", &now), "1m");
    }

    #[test]
    fn countdown_exactly_one_hour() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-12T19:00:00Z", &now), "1h 0m");
    }

    #[test]
    fn countdown_exactly_one_day() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(fmt_countdown("2026-07-13T18:00:00Z", &now), "1d 0h");
    }

    // ---- next_reset_countdown ----

    #[test]
    fn next_reset_picks_five_hour_when_sooner() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mut u = empty_usage();
        u.five_hour = Some(window(36.0, "2026-07-12T20:00:00Z"));
        u.seven_day = Some(window(11.0, "2026-07-17T00:00:00Z"));
        assert_eq!(next_reset_countdown(&u, &now), "2h 0m");
    }

    #[test]
    fn next_reset_picks_seven_day_when_no_five_hour() {
        let now = "2026-07-12T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mut u = empty_usage();
        u.seven_day = Some(window(11.0, "2026-07-17T00:00:00Z"));
        assert_eq!(next_reset_countdown(&u, &now), "4d 6h");
    }

    #[test]
    fn next_reset_no_windows() {
        let now = Utc::now();
        assert_eq!(next_reset_countdown(&empty_usage(), &now), "-");
    }

    #[test]
    fn next_reset_both_past() {
        let now = "2026-07-20T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mut u = empty_usage();
        u.five_hour = Some(window(36.0, "2026-07-12T20:00:00Z"));
        u.seven_day = Some(window(11.0, "2026-07-17T00:00:00Z"));
        assert_eq!(next_reset_countdown(&u, &now), "-");
    }

    // ---- severity_key ----

    #[test]
    fn test_severity_key() {
        assert_eq!(severity_key("session"), "5H");
        assert_eq!(severity_key("weekly_all"), "WEEK");
        assert_eq!(severity_key("custom"), "custom");
    }

    // ---- parse_iso ----

    #[test]
    fn test_parse_iso_valid() {
        assert!(parse_iso("2026-07-12T18:40:00Z").is_some());
        assert!(parse_iso("2026-07-12T18:40:00+00:00").is_some());
        assert!(parse_iso("2026-07-12T20:40:00+02:00").is_some());
    }

    #[test]
    fn test_parse_iso_invalid() {
        assert!(parse_iso("").is_none());
        assert!(parse_iso("not-a-date").is_none());
        assert!(parse_iso("2026-07-12").is_none());
    }
}
