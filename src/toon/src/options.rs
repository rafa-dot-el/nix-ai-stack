/// The active cell/row separator. TOON's header syntax records a non-default
/// choice inline (e.g. `items[3|]:` for [`Delimiter::Pipe`]); comma needs no
/// such marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delimiter {
    Comma,
    Tab,
    Pipe,
}

impl Delimiter {
    pub(crate) fn as_char(self) -> char {
        match self {
            Delimiter::Comma => ',',
            Delimiter::Tab => '\t',
            Delimiter::Pipe => '|',
        }
    }

    pub(crate) fn header_suffix(self) -> &'static str {
        match self {
            Delimiter::Comma => "",
            Delimiter::Tab => "\t",
            Delimiter::Pipe => "|",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EncodeOptions {
    pub delimiter: Delimiter,
    pub indent_width: usize,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            delimiter: Delimiter::Comma,
            indent_width: 2,
        }
    }
}

impl EncodeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_delimiter(mut self, delimiter: Delimiter) -> Self {
        self.delimiter = delimiter;
        self
    }

    pub fn with_indent(mut self, width: usize) -> Self {
        self.indent_width = width;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DecodeOptions {
    pub strict: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self { strict: true }
    }
}

impl DecodeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}
