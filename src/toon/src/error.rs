use thiserror::Error;

/// Errors returned by [`crate::encode`]/[`crate::decode`]. See
/// `specs/toon-format/spec.md` (add-mcp-tool-pipeline change) for the
/// behavior contract each variant backs.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToonError {
    #[error("array `{key}` declares length {expected} but found {found} rows")]
    LengthMismatch {
        key: String,
        expected: usize,
        found: usize,
    },
    #[error("array `{key}` declares {expected} field(s) but a row has {found} value(s)")]
    FieldCountMismatch {
        key: String,
        expected: usize,
        found: usize,
    },
    #[error("invalid TOON structure: {0}")]
    InvalidStructure(String),
    #[error("failed to convert value: {0}")]
    Conversion(String),
}
