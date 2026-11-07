use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageData {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub seven_day_oauth_apps: Option<Window>,
    pub seven_day_opus: Option<Window>,
    pub seven_day_sonnet: Option<Window>,
    pub seven_day_cowork: Option<Window>,
    pub seven_day_omelette: Option<Window>,
    pub tangelo: Option<Window>,
    pub iguana_necktie: Option<Window>,
    pub omelette_promotional: Option<Window>,
    pub nimbus_quill: Option<Window>,
    pub cinder_cove: Option<Window>,
    pub amber_ladder: Option<Window>,
    pub extra_usage: Option<ExtraUsage>,
    pub limits: Option<Vec<Limit>>,
    pub spend: Option<Spend>,
    pub member_dashboard_available: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub utilization: Option<f64>,
    pub resets_at: Option<String>,
    pub limit_dollars: Option<f64>,
    pub used_dollars: Option<f64>,
    pub remaining_dollars: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limit {
    pub kind: String,
    pub group: Option<String>,
    pub percent: Option<f64>,
    pub severity: Option<String>,
    pub resets_at: Option<String>,
    pub scope: Option<LimitScope>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitScope {
    pub model: Option<ScopeModel>,
    pub surface: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeModel {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtraUsage {
    pub is_enabled: Option<bool>,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
    pub currency: Option<String>,
    pub decimal_places: Option<u32>,
    pub disabled_reason: Option<String>,
    pub daily: Option<serde_json::Value>,
    pub weekly: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spend {
    pub used: Option<SpendAmount>,
    pub limit: Option<serde_json::Value>,
    pub percent: Option<f64>,
    pub severity: Option<String>,
    pub enabled: Option<bool>,
    pub disabled_reason: Option<String>,
    pub cap: Option<serde_json::Value>,
    pub balance: Option<serde_json::Value>,
    pub auto_reload: Option<serde_json::Value>,
    pub disclaimer: Option<String>,
    pub can_purchase_credits: Option<bool>,
    pub can_toggle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendAmount {
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub exponent: Option<u32>,
}
