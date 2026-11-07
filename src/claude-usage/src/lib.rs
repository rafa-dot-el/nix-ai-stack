//! claude-usage: Claude Code's plan usage (the OAuth usage endpoint) as a table, a polybar
//! format string or Prometheus metrics. Pure: model, vars, format, table, exporter::metrics,
//! creds::token_at; effects: the credentials file (read only, never written), the HTTP
//! client, the exporter's listener.
pub mod client;
pub mod config;
pub mod creds;
pub mod exporter;
pub mod format;
pub mod model;
pub mod table;
pub mod vars;
