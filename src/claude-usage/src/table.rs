//! The terminal table (pure): plain, or with ANSI colours.
use std::collections::HashMap;

use crate::model::UsageData;

const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn paint(s: &str, code: &str, colour: bool) -> String {
    if colour { format!("{code}{s}{RESET}") } else { s.to_string() }
}

fn tone(pct: f64) -> &'static str {
    if pct >= 80.0 {
        "\x1b[31m"
    } else if pct >= 60.0 {
        "\x1b[33m"
    } else {
        "\x1b[32m"
    }
}

fn severity_tone(sev: &str) -> &'static str {
    match sev {
        "warning" => "\x1b[33m",
        "critical" => "\x1b[31m",
        _ => "\x1b[32m",
    }
}

fn bar(pct: f64, width: usize) -> String {
    let filled = (((pct / 100.0) * width as f64).round().max(0.0) as usize).min(width);
    format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(width - filled))
}

fn row(out: &mut String, label: &str, vars: &HashMap<String, String>, key: &str, colour: bool) {
    let get = |k: String| vars.get(&k).map(String::as_str);
    let pct = get(format!("PCT_{key}")).unwrap_or("-");
    let num: f64 = pct.parse().unwrap_or(0.0);
    let sev = get(format!("SEV_{key}")).unwrap_or("unknown");
    out.push_str(&format!(
        "  {:<18} {}  {}  resets {} ({})\n",
        label,
        paint(&format!("{pct:>3}%"), severity_tone(sev), colour),
        paint(&bar(num, 20), tone(num), colour),
        get(format!("RESET_{key}")).unwrap_or("-"),
        get(format!("COUNTDOWN_{key}")).unwrap_or("-"),
    ));
}

pub fn table(usage: &UsageData, vars: &HashMap<String, String>, colour: bool) -> String {
    let mut out = format!("{}\n{}\n", paint("Claude Code Usage", BOLD, colour), "\u{2500}".repeat(60));
    row(&mut out, "5-hour session", vars, "5H", colour);
    row(&mut out, "7-day overall", vars, "WEEK", colour);
    for limit in usage.limits.iter().flatten() {
        if let Some(name) = limit.scope.as_ref().and_then(|s| s.model.as_ref()).and_then(|m| m.display_name.as_ref()) {
            row(&mut out, &format!("Weekly ({name})"), vars, &name.to_uppercase().replace(' ', "_"), colour);
        }
    }
    out
}
