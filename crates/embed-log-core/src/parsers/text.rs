use super::traits::StreamParser;

/// Newline-delimited UTF-8 text parser.
///
/// Splits incoming bytes on `\n` and returns complete lines.  Partial lines
/// are buffered until the next newline arrives.
pub struct TextParser {
    buf: Vec<u8>,
}

impl TextParser {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }
}

impl Default for TextParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamParser for TextParser {
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in data {
            if byte == b'\n' {
                let line = String::from_utf8_lossy(&self.buf).trim_end().to_string();
                self.buf.clear();
                if !line.is_empty() {
                    lines.push(line);
                }
            } else {
                self.buf.push(byte);
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let mut p = TextParser::new();
        let lines = p.feed(b"hello world\n");
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn multiple_lines() {
        let mut p = TextParser::new();
        let lines = p.feed(b"line1\nline2\nline3\n");
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn partial_lines() {
        let mut p = TextParser::new();
        let lines = p.feed(b"part1");
        assert!(lines.is_empty());
        let lines = p.feed(b" part2\n");
        assert_eq!(lines, vec!["part1 part2"]);
    }

    #[test]
    fn empty_lines_skipped() {
        let mut p = TextParser::new();
        let lines = p.feed(b"\n\n\n");
        assert!(lines.is_empty());
    }

    #[test]
    fn trailing_cr_stripped() {
        let mut p = TextParser::new();
        let lines = p.feed(b"line\r\n");
        assert_eq!(lines, vec!["line"]);
    }
}
