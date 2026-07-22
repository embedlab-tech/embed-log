use serde_json::Value;

/// One parsed line, with optional lossless source text and structured metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLine {
    /// Text shown to live consumers and used for decoded artifacts.
    pub display: String,
    /// Original source text when parsing transformed the display text.
    pub raw: Option<String>,
    /// Parser-specific structured data included in WebSocket/JSONL records.
    pub meta: Option<Value>,
}

impl From<String> for ParsedLine {
    fn from(display: String) -> Self {
        Self {
            display,
            raw: None,
            meta: None,
        }
    }
}

/// Trait for stream parsers that convert raw bytes into text lines.
pub trait StreamParser: Send + 'static {
    /// Feed raw bytes and return decoded text lines.
    ///
    /// Implementations may buffer partial data internally (e.g. incomplete
    /// CBOR packets or partial lines) and return complete lines only.
    fn feed(&mut self, data: &[u8]) -> Vec<String>;

    /// Feed raw bytes while preserving source text and parser metadata.
    /// Parsers that only produce text use the default conversion.
    fn feed_entries(&mut self, data: &[u8]) -> Vec<ParsedLine> {
        self.feed(data).into_iter().map(ParsedLine::from).collect()
    }
}
