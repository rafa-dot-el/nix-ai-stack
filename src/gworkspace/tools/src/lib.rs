//! The gworkspace binaries (gtasks first). Each is a CLI and an MCP server over the same
//! client code; credentials and endpoints come from ~/.config/rafa/gworkspace.yaml.
pub mod calendar;
pub mod cli;
pub mod config;
pub mod context;
pub mod docs;
pub mod drive;
pub mod mail;
pub mod meet;
pub mod output;
pub mod sheets;
pub mod tasks;
