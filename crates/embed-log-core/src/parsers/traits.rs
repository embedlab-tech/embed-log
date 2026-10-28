/// Trait for stream parsers that convert raw bytes into text lines.
pub trait StreamParser: Send + 'static {
    /// Feed raw bytes and return decoded text lines.
    ///
    /// Implementations may buffer partial data internally (e.g. incomplete
    /// datagrams or partial lines) and return complete lines only.
    fn feed(&mut self, data: &[u8]) -> Vec<String>;

    /// Called repeatedly by UART sources during sustained silence. `idle_for`
    /// is monotonic elapsed time since the last received byte, not the last call.
    /// Silence alone does not prove truncation; parsers may flush console text
    /// or apply an explicit, bounded abandonment policy for incomplete records.
    fn on_idle(&mut self, _idle_for: std::time::Duration) -> Vec<String> {
        Vec::new()
    }
}
