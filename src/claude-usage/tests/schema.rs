use claude_usage::model::*;

fn load_fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn load_usage(name: &str) -> UsageData {
    serde_json::from_str(&load_fixture(name)).unwrap_or_else(|e| panic!("parse {name}: {e}"))
}

// ---------------------------------------------------------------------------
// Contract snapshot: keeps `usage_full.json` in lock-step with `UsageData`.
//
// This list must mirror both `UsageData`'s field names (src/model.rs) and
// `KNOWN_TOP_LEVEL_KEYS` in tests/live_api.rs. It runs with plain `cargo
// test` (no network/credentials needed), so a model change without a
// matching fixture/live-test update fails fast, before `task test:e2e`
// against the real API ever runs.
// ---------------------------------------------------------------------------

const KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
    "five_hour",
    "seven_day",
    "seven_day_oauth_apps",
    "seven_day_opus",
    "seven_day_sonnet",
    "seven_day_cowork",
    "seven_day_omelette",
    "tangelo",
    "iguana_necktie",
    "omelette_promotional",
    "nimbus_quill",
    "cinder_cove",
    "amber_ladder",
    "extra_usage",
    "limits",
    "spend",
    "member_dashboard_available",
];

#[test]
fn usage_full_fixture_matches_known_contract_exactly() {
    let raw: serde_json::Value = serde_json::from_str(&load_fixture("usage_full.json")).unwrap();
    let obj = raw.as_object().unwrap();

    let fixture_keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let known_keys: std::collections::BTreeSet<&str> =
        KNOWN_TOP_LEVEL_KEYS.iter().copied().collect();

    assert_eq!(
        fixture_keys, known_keys,
        "usage_full.json top-level keys diverged from the known contract. \
         If you added a field to UsageData, add it here (and to \
         tests/live_api.rs::KNOWN_TOP_LEVEL_KEYS) and give it a realistic \
         value in the fixture."
    );
}

#[test]
fn usage_full_fixture_includes_scoped_model_limit_example() {
    let u = load_usage("usage_full.json");
    let limits = u.limits.as_ref().unwrap();
    let scoped = limits.iter().find(|l| l.kind == "weekly_model");
    assert!(
        scoped.is_some(),
        "usage_full.json should keep a weekly_model example so per-model \
         limits (Fable, Opus, etc.) stay covered even when no live account \
         currently has one active"
    );
    let model = scoped
        .unwrap()
        .scope
        .as_ref()
        .unwrap()
        .model
        .as_ref()
        .unwrap();
    assert!(model.display_name.is_some());
}

// ---------------------------------------------------------------------------
// Deserialization from fixtures
// ---------------------------------------------------------------------------

#[test]
fn deserialize_full_fixture() {
    let u = load_usage("usage_full.json");
    assert!(u.five_hour.is_some());
    assert!(u.seven_day.is_some());
    assert!(u.limits.is_some());
    assert!(u.spend.is_some());
    assert!(u.extra_usage.is_some());
    assert_eq!(u.member_dashboard_available, Some(true));
}

#[test]
fn deserialize_minimal_fixture() {
    let u = load_usage("usage_minimal.json");
    assert!(u.five_hour.is_none());
    assert!(u.seven_day.is_none());
    assert!(u.limits.is_none());
    assert!(u.spend.is_none());
    assert!(u.extra_usage.is_none());
    assert!(u.member_dashboard_available.is_none());
}

#[test]
fn deserialize_warning_fixture() {
    let u = load_usage("usage_warning.json");
    let limits = u.limits.as_ref().unwrap();
    assert!(limits
        .iter()
        .all(|l| l.severity.as_deref() == Some("warning")));
}

#[test]
fn deserialize_critical_fixture() {
    let u = load_usage("usage_critical.json");
    let limits = u.limits.as_ref().unwrap();
    assert!(limits
        .iter()
        .all(|l| l.severity.as_deref() == Some("critical")));
}

#[test]
fn deserialize_multi_model_fixture() {
    let u = load_usage("usage_multi_model.json");
    let limits = u.limits.as_ref().unwrap();
    let scoped: Vec<_> = limits.iter().filter(|l| l.scope.is_some()).collect();
    assert_eq!(scoped.len(), 3);

    let names: Vec<_> = scoped
        .iter()
        .filter_map(|l| l.scope.as_ref()?.model.as_ref()?.display_name.as_deref())
        .collect();
    assert!(names.contains(&"Fable"));
    assert!(names.contains(&"Opus 3"));
    assert!(names.contains(&"Sonnet 4"));
}

// ---------------------------------------------------------------------------
// Roundtrip: serialize -> deserialize
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_full() {
    let original = load_usage("usage_full.json");
    let json = serde_json::to_string(&original).unwrap();
    let roundtripped: UsageData = serde_json::from_str(&json).unwrap();

    assert_eq!(
        original.five_hour.as_ref().unwrap().utilization,
        roundtripped.five_hour.as_ref().unwrap().utilization,
    );
    assert_eq!(
        original.limits.as_ref().unwrap().len(),
        roundtripped.limits.as_ref().unwrap().len(),
    );
}

#[test]
fn roundtrip_minimal() {
    let original = load_usage("usage_minimal.json");
    let json = serde_json::to_string(&original).unwrap();
    let roundtripped: UsageData = serde_json::from_str(&json).unwrap();
    assert!(roundtripped.five_hour.is_none());
    assert!(roundtripped.limits.is_none());
}

// ---------------------------------------------------------------------------
// Forward compatibility: unknown fields must not break deserialization
// ---------------------------------------------------------------------------

#[test]
fn unknown_top_level_fields_tolerated() {
    let json = r#"{
        "five_hour": null, "seven_day": null,
        "seven_day_oauth_apps": null, "seven_day_opus": null,
        "seven_day_sonnet": null, "seven_day_cowork": null,
        "seven_day_omelette": null, "tangelo": null,
        "iguana_necktie": null, "omelette_promotional": null,
        "nimbus_quill": null, "cinder_cove": null,
        "amber_ladder": null, "extra_usage": null,
        "limits": null, "spend": null,
        "member_dashboard_available": null,
        "brand_new_field_2027": "surprise",
        "another_future_field": 42
    }"#;
    let u: UsageData = serde_json::from_str(json).unwrap();
    assert!(u.five_hour.is_none());
}

#[test]
fn unknown_window_fields_tolerated() {
    let json = r#"{
        "five_hour": {
            "utilization": 50.0,
            "resets_at": "2026-07-12T20:00:00Z",
            "limit_dollars": null,
            "used_dollars": null,
            "remaining_dollars": null,
            "new_window_metric": true
        },
        "seven_day": null,
        "seven_day_oauth_apps": null, "seven_day_opus": null,
        "seven_day_sonnet": null, "seven_day_cowork": null,
        "seven_day_omelette": null, "tangelo": null,
        "iguana_necktie": null, "omelette_promotional": null,
        "nimbus_quill": null, "cinder_cove": null,
        "amber_ladder": null, "extra_usage": null,
        "limits": null, "spend": null,
        "member_dashboard_available": null
    }"#;
    let u: UsageData = serde_json::from_str(json).unwrap();
    assert_eq!(u.five_hour.as_ref().unwrap().utilization, Some(50.0));
}

#[test]
fn unknown_limit_fields_tolerated() {
    let json = r#"{
        "five_hour": null, "seven_day": null,
        "seven_day_oauth_apps": null, "seven_day_opus": null,
        "seven_day_sonnet": null, "seven_day_cowork": null,
        "seven_day_omelette": null, "tangelo": null,
        "iguana_necktie": null, "omelette_promotional": null,
        "nimbus_quill": null, "cinder_cove": null,
        "amber_ladder": null, "extra_usage": null,
        "limits": [{
            "kind": "session",
            "percent": 10.0,
            "severity": "normal",
            "future_limit_field": [1,2,3]
        }],
        "spend": null,
        "member_dashboard_available": null
    }"#;
    let u: UsageData = serde_json::from_str(json).unwrap();
    assert_eq!(u.limits.as_ref().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// Window struct validation
// ---------------------------------------------------------------------------

#[test]
fn window_all_fields_none() {
    let json = r#"{"utilization": null, "resets_at": null, "limit_dollars": null, "used_dollars": null, "remaining_dollars": null}"#;
    let w: Window = serde_json::from_str(json).unwrap();
    assert!(w.utilization.is_none());
    assert!(w.resets_at.is_none());
}

#[test]
fn window_utilization_zero() {
    let json = r#"{"utilization": 0.0, "resets_at": null, "limit_dollars": null, "used_dollars": null, "remaining_dollars": null}"#;
    let w: Window = serde_json::from_str(json).unwrap();
    assert_eq!(w.utilization, Some(0.0));
}

#[test]
fn window_utilization_100() {
    let json = r#"{"utilization": 100.0, "resets_at": null, "limit_dollars": null, "used_dollars": null, "remaining_dollars": null}"#;
    let w: Window = serde_json::from_str(json).unwrap();
    assert_eq!(w.utilization, Some(100.0));
}

#[test]
fn window_utilization_over_100() {
    let json = r#"{"utilization": 105.5, "resets_at": null, "limit_dollars": null, "used_dollars": null, "remaining_dollars": null}"#;
    let w: Window = serde_json::from_str(json).unwrap();
    assert_eq!(w.utilization, Some(105.5));
}

#[test]
fn window_utilization_fractional() {
    let json = r#"{"utilization": 33.333, "resets_at": null, "limit_dollars": null, "used_dollars": null, "remaining_dollars": null}"#;
    let w: Window = serde_json::from_str(json).unwrap();
    assert!((w.utilization.unwrap() - 33.333).abs() < 0.001);
}

#[test]
fn window_dollars_consistency() {
    let u = load_usage("usage_full.json");
    let w = u.five_hour.as_ref().unwrap();
    let limit = w.limit_dollars.unwrap();
    let used = w.used_dollars.unwrap();
    let remaining = w.remaining_dollars.unwrap();
    assert!((used + remaining - limit).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// Limit struct validation
// ---------------------------------------------------------------------------

#[test]
fn limit_kind_values() {
    let u = load_usage("usage_full.json");
    let limits = u.limits.as_ref().unwrap();
    let kinds: Vec<_> = limits.iter().map(|l| l.kind.as_str()).collect();
    assert!(kinds.contains(&"session"));
    assert!(kinds.contains(&"weekly_all"));
    assert!(kinds.contains(&"weekly_model"));
}

#[test]
fn limit_severity_valid_values() {
    let severities = ["normal", "warning", "critical"];
    for sev in severities {
        let json = format!(r#"{{"kind":"session","percent":50.0,"severity":"{sev}"}}"#);
        let l: Limit = serde_json::from_str(&json).unwrap();
        assert_eq!(l.severity.as_deref(), Some(sev));
    }
}

#[test]
fn limit_severity_null() {
    let json = r#"{"kind":"session","percent":50.0,"severity":null}"#;
    let l: Limit = serde_json::from_str(json).unwrap();
    assert!(l.severity.is_none());
}

#[test]
fn limit_scope_model_full() {
    let json = r#"{"kind":"weekly_model","percent":20.0,"scope":{"model":{"id":"fable","display_name":"Fable"},"surface":null}}"#;
    let l: Limit = serde_json::from_str(json).unwrap();
    let scope = l.scope.unwrap();
    let model = scope.model.unwrap();
    assert_eq!(model.id.as_deref(), Some("fable"));
    assert_eq!(model.display_name.as_deref(), Some("Fable"));
}

#[test]
fn limit_scope_model_no_display_name() {
    let json = r#"{"kind":"weekly_model","percent":10.0,"scope":{"model":{"id":"unknown-model"},"surface":null}}"#;
    let l: Limit = serde_json::from_str(json).unwrap();
    let model = l.scope.unwrap().model.unwrap();
    assert!(model.display_name.is_none());
    assert_eq!(model.id.as_deref(), Some("unknown-model"));
}

#[test]
fn limit_scope_with_surface() {
    let u = load_usage("usage_multi_model.json");
    let limits = u.limits.as_ref().unwrap();
    let with_surface: Vec<_> = limits
        .iter()
        .filter(|l| l.scope.as_ref().and_then(|s| s.surface.as_ref()).is_some())
        .collect();
    assert_eq!(with_surface.len(), 1);
    assert_eq!(
        with_surface[0].scope.as_ref().unwrap().surface.as_deref(),
        Some("code")
    );
}

#[test]
fn limit_percent_range() {
    let u = load_usage("usage_full.json");
    for limit in u.limits.as_ref().unwrap() {
        if let Some(pct) = limit.percent {
            assert!(pct >= 0.0, "percent should be non-negative: {pct}");
        }
    }
}

// ---------------------------------------------------------------------------
// ExtraUsage struct
// ---------------------------------------------------------------------------

#[test]
fn extra_usage_full() {
    let u = load_usage("usage_full.json");
    let eu = u.extra_usage.as_ref().unwrap();
    assert_eq!(eu.is_enabled, Some(false));
    assert_eq!(eu.currency.as_deref(), Some("USD"));
    assert_eq!(eu.decimal_places, Some(2));
    assert_eq!(eu.disabled_reason.as_deref(), Some("not_enrolled"));
}

#[test]
fn extra_usage_null() {
    let u = load_usage("usage_minimal.json");
    assert!(u.extra_usage.is_none());
}

// ---------------------------------------------------------------------------
// Spend struct
// ---------------------------------------------------------------------------

#[test]
fn spend_full() {
    let u = load_usage("usage_full.json");
    let sp = u.spend.as_ref().unwrap();
    assert_eq!(sp.enabled, Some(false));
    assert_eq!(sp.percent, Some(0.0));
    assert_eq!(sp.severity.as_deref(), Some("normal"));
    assert_eq!(sp.can_purchase_credits, Some(true));
    assert_eq!(sp.can_toggle, Some(true));
    assert!(sp.disclaimer.is_some());

    let used = sp.used.as_ref().unwrap();
    assert_eq!(used.amount_minor, Some(0));
    assert_eq!(used.currency.as_deref(), Some("USD"));
    assert_eq!(used.exponent, Some(2));
}

#[test]
fn spend_null() {
    let u = load_usage("usage_minimal.json");
    assert!(u.spend.is_none());
}

// ---------------------------------------------------------------------------
// resets_at ISO-8601 format validation
// ---------------------------------------------------------------------------

#[test]
fn resets_at_parses_as_rfc3339() {
    let u = load_usage("usage_full.json");
    let ts = u.five_hour.as_ref().unwrap().resets_at.as_ref().unwrap();
    assert!(
        chrono::DateTime::parse_from_rfc3339(ts).is_ok(),
        "resets_at should be valid RFC3339: {ts}"
    );
}

#[test]
fn all_resets_at_parse() {
    for fixture in [
        "usage_full.json",
        "usage_warning.json",
        "usage_critical.json",
        "usage_multi_model.json",
    ] {
        let u = load_usage(fixture);
        for w in [&u.five_hour, &u.seven_day].into_iter().flatten() {
            if let Some(ref ts) = w.resets_at {
                assert!(
                    chrono::DateTime::parse_from_rfc3339(ts).is_ok(),
                    "{fixture}: window resets_at invalid: {ts}"
                );
            }
        }
        if let Some(ref limits) = u.limits {
            for (i, l) in limits.iter().enumerate() {
                if let Some(ref ts) = l.resets_at {
                    assert!(
                        chrono::DateTime::parse_from_rfc3339(ts).is_ok(),
                        "{fixture}: limits[{i}].resets_at invalid: {ts}"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Schema shape: bare-minimum required fields
// ---------------------------------------------------------------------------

#[test]
fn empty_object_fails() {
    let result = serde_json::from_str::<UsageData>("{}");
    // All top-level fields are Option so this should succeed
    assert!(result.is_ok());
}

#[test]
fn limit_requires_kind() {
    let json = r#"{"percent": 50.0, "severity": "normal"}"#;
    let result = serde_json::from_str::<Limit>(json);
    assert!(result.is_err(), "Limit without 'kind' should fail");
}

// ---------------------------------------------------------------------------
// Cross-fixture invariants
// ---------------------------------------------------------------------------

#[test]
fn severity_escalation_ordering() {
    let normal = load_usage("usage_full.json");
    let warning = load_usage("usage_warning.json");
    let critical = load_usage("usage_critical.json");

    let pct_n = normal.five_hour.as_ref().unwrap().utilization.unwrap();
    let pct_w = warning.five_hour.as_ref().unwrap().utilization.unwrap();
    let pct_c = critical.five_hour.as_ref().unwrap().utilization.unwrap();

    assert!(pct_n < pct_w, "normal < warning utilization");
    assert!(pct_w < pct_c, "warning < critical utilization");
}

#[test]
fn all_fixtures_deserialize() {
    for name in [
        "usage_full.json",
        "usage_minimal.json",
        "usage_warning.json",
        "usage_critical.json",
        "usage_multi_model.json",
    ] {
        let _ = load_usage(name);
    }
}
