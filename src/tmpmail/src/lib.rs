//! tmpmail: disposable GuerrillaMail inboxes for tests and sign-up flows. Pure: `format`,
//! `identities` (the store's logic), `api::id_text` and the request shapes; effects: the HTTP
//! client, the identities file, `ops` (what the CLI and the MCP server both do), `mcp`.
pub mod api;
pub mod format;
pub mod identities;
pub mod mcp;
pub mod ops;
pub mod setup;
