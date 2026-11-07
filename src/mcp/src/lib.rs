//! `mcp`: one MCP entry point for every agent (ADR-0004).
//!
//! agent ─stdio─▶ `mcp client` ─unix─▶ `mcp proxy` ─unix─▶ servers (socket-activated units)
//!
//! [`config`] is pure; the async parts live in [`proxy`], [`client`] and [`sidecar`].
pub mod app;
pub mod config;
pub mod doctor;
pub mod edit;
pub mod proxy;
pub mod relay;
pub mod sidecar;
pub mod system;
pub mod tui;
pub mod cli;
pub mod client;
