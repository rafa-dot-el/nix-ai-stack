//! How many tokens a tool definition costs an agent, and what a filter saves (ADR-0004).
//!
//! The estimate is tiktoken's `o200k_base` over the tool's canonical JSON
//! (`{"name", "description", "inputSchema"}`), the method of a private predecessor's mcp-filter TUI
//! used. It is an estimate: every model family tokenises differently. Offline and pure.

use std::sync::OnceLock;

use serde_json::{json, Value};
use tiktoken_rs::CoreBPE;

fn bpe() -> &'static CoreBPE {
    static BPE: OnceLock<CoreBPE> = OnceLock::new();
    // The o200k_base vocabulary ships inside tiktoken-rs; loading it cannot fail at runtime.
    BPE.get_or_init(|| tiktoken_rs::o200k_base().expect("bundled o200k_base vocabulary"))
}

/// Tokens of one tool definition as an agent receives it.
pub fn tool_tokens(name: &str, description: Option<&str>, input_schema: &Value) -> usize {
    let definition = json!({ "name": name, "description": description, "inputSchema": input_schema });
    bpe().encode_ordinary(&definition.to_string()).len()
}

/// Totals for a server or a profile: every tool, the ones a filter keeps, and the share saved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Savings {
    pub full: usize,
    pub filtered: usize,
    /// 0.0 to 100.0; 0.0 when there is nothing to save.
    pub percent_saved: f64,
}

/// `all`: tokens of every tool offered; `kept`: tokens of the tools the filter exposes.
pub fn savings(all: &[usize], kept: &[usize]) -> Savings {
    let full: usize = all.iter().sum();
    let filtered: usize = kept.iter().sum();
    let percent_saved = if full == 0 { 0.0 } else { (full - filtered.min(full)) as f64 * 100.0 / full as f64 };
    Savings { full, filtered, percent_saved }
}
