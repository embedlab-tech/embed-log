/// Trait for stream parsers that convert raw bytes into text lines.
pub trait StreamParser: Send + 'static {
    /// Feed raw bytes and return decoded text lines.
    ///
    /// Implementations may buffer partial data internally (e.g. incomplete
    /// CBOR packets or partial lines) and return complete lines only.
    fn feed(&mut self, data: &[u8]) -> Vec<String>;
}
