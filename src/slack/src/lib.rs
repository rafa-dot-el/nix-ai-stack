//! slack: one Slack client for the CLI and the MCP server (plan 5). The pure core decides
//! what to send and where it may go; the effectful edge talks to Slack.
pub mod actions;
pub mod analysis;
pub mod blocks;
pub mod client;
pub mod config;
pub mod mcp;
pub mod policy;
pub mod prepare;
pub mod read;
pub mod send;
pub mod setup;
pub mod spec;
pub mod target;
pub mod wire;
