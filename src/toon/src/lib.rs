//! Vendored, independently-maintained TOON (Token-Oriented Object Notation)
//! codec for `serde_json::Value`, plus a JSON-Schema-to-TOON-header preview
//! synthesizer. Ported from `toon-format/toon-rust` v0.5.0 (MIT) and owned
//! here rather than pulled from the registry. No MCP-specific types anywhere in
//! `toon-format` rather than depend on the upstream crate" decision
//! (add-mcp-tool-pipeline change). No MCP-specific types anywhere in this
//! crate by design, so it stays reusable outside the MCP layer.

mod decode;
mod encode;
mod error;
mod options;
mod preview;

pub use error::ToonError;
pub use options::{DecodeOptions, Delimiter, EncodeOptions};
pub use preview::schema_preview;

use serde_json::Value;

/// Encodes `value` to TOON text.
pub fn encode(value: &Value, opts: &EncodeOptions) -> Result<String, ToonError> {
    encode::encode(value, opts)
}

/// Encodes `value` to TOON text using [`EncodeOptions::default`].
pub fn encode_default(value: &Value) -> Result<String, ToonError> {
    encode(value, &EncodeOptions::default())
}

/// Decodes TOON text into a [`Value`].
pub fn decode(input: &str, opts: &DecodeOptions) -> Result<Value, ToonError> {
    decode::decode(input, opts)
}

/// Decodes TOON text into a [`Value`] using [`DecodeOptions::default`].
pub fn decode_default(input: &str) -> Result<Value, ToonError> {
    decode(input, &DecodeOptions::default())
}
