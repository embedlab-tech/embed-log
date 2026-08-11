use super::slip_coap::coap;
use super::{StreamParser, TextParser};

/// Newline-delimited text parser that replaces the first valid hexadecimal
/// CoAP message in each line with a human-readable decode.
///
/// Text before the first CoAP byte is retained verbatim. The hexadecimal
/// message and anything following it are replaced by the decoded form, which
/// makes prefixed captures such as `radio rx: 40 01 ...` directly readable.
pub struct HexCoapParser {
    text: TextParser,
}

impl HexCoapParser {
    pub fn new() -> Self {
        Self {
            text: TextParser::new(),
        }
    }
}

impl Default for HexCoapParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamParser for HexCoapParser {
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        self.text
            .feed(data)
            .into_iter()
            .map(|line| decode_line(&line).unwrap_or(line))
            .collect()
    }
}

fn decode_line(line: &str) -> Option<String> {
    // Structured capture rows conventionally put their wire payload in the
    // final pipe-delimited field. When that field is hexadecimal, it is the
    // sole CoAP candidate: metadata must never contribute nibbles or win as
    // an earlier false-positive CoAP-looking value.
    if let Some((payload_offset, payload)) = final_pipe_hex_payload(line) {
        return coap::parse(&payload)
            .map(|decoded| format!("{}[CoAP]{}", &line[..payload_offset], decoded));
    }

    // Preserve generic log-line behavior for non-structured inputs, such as
    // `radio rx: 40 01 ...`, where a hexadecimal packet follows free text.
    for (run_start, run_end) in candidate_runs(line) {
        let run = &line[run_start..run_end];
        let mut nibbles = String::new();
        let mut offsets = Vec::new();
        for (offset, character) in run.char_indices() {
            if character.is_ascii_hexdigit() {
                nibbles.push(character);
                offsets.push(run_start + offset);
            }
        }
        if nibbles.len() < 8 {
            continue;
        }
        // Try every nibble rather than assuming the surrounding log prefix is
        // byte-aligned. This handles text ending in A-F immediately before a
        // separated dump while still selecting the first valid CoAP header.
        for nibble_start in 0..=nibbles.len() - 8 {
            let remaining = nibbles.len() - nibble_start;
            if remaining % 2 != 0 {
                continue;
            }
            let bytes = hex_bytes(&nibbles[nibble_start..])?;
            let Some(decoded) = coap::parse(&bytes) else {
                continue;
            };
            let coap_offset = offsets[nibble_start];
            return Some(format!("{}[CoAP]{}", &line[..coap_offset], decoded));
        }
    }
    None
}

fn final_pipe_hex_payload(line: &str) -> Option<(usize, Vec<u8>)> {
    let (_, final_field) = line.rsplit_once('|')?;
    let trimmed = final_field.trim();
    if trimmed.len() < 8
        || !trimmed
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character.is_ascii_whitespace())
    {
        return None;
    }
    let hex: String = trimmed
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .collect();
    if hex.len() < 8 || hex.len() % 2 != 0 {
        return None;
    }
    let payload_offset =
        line.len() - final_field.len() + (final_field.len() - final_field.trim_start().len());
    Some((payload_offset, hex_bytes(&hex)?))
}

fn candidate_runs(line: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        let allowed = character.is_ascii_hexdigit()
            || character.is_ascii_whitespace()
            || matches!(character, ':' | ',' | '_' | '.' | '-');
        match (start, allowed) {
            (None, true) if character.is_ascii_hexdigit() => start = Some(index),
            (Some(run_start), false) => {
                runs.push((run_start, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(run_start) = start {
        runs.push((run_start, line.len()));
    }
    runs
}

fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COAP_GET_FOO_BAR: &str = "40011234b3666f6f03626172";

    #[test]
    fn compact_hex_is_replaced_with_human_readable_coap() {
        let mut parser = HexCoapParser::new();
        let lines = parser.feed(format!("{COAP_GET_FOO_BAR}\n").as_bytes());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("[CoAP] t:CON c:GET"), "{}", lines[0]);
        assert!(lines[0].contains("i:1234"));
        assert!(lines[0].contains("Uri-Path: foo"));
        assert!(lines[0].contains("Uri-Path: bar"));
        assert!(!lines[0].contains(COAP_GET_FOO_BAR));
    }

    #[test]
    fn separated_hex_keeps_prefix_and_starts_at_first_valid_coap_header() {
        let mut parser = HexCoapParser::new();
        let line = "radio rx frame aa 55 payload 40 01 12 34 b3 66 6f 6f 03 62 61 72 suffix\n";
        let lines = parser.feed(line.as_bytes());
        assert!(lines[0].starts_with("radio rx frame aa 55 payload [CoAP] t:CON c:GET"));
        assert!(!lines[0].contains("40 01 12 34"));
        assert!(!lines[0].contains("suffix"));
    }

    #[test]
    fn pipe_delimited_capture_uses_only_final_hex_payload() {
        let mut parser = HexCoapParser::new();
        let earlier_coap = "40011234b3666f6f03626172";
        let payload = "480212340102030405060708";
        // The metadata deliberately contains a valid CoAP GET. Only the final
        // synthetic POST payload may be decoded.
        let line = format!(
            "2026-01-02 03:04:05.006 | udp | {earlier_coap} | tx | 49152 | 5683 | {payload}\n"
        );
        let lines = parser.feed(line.as_bytes());
        assert!(
            lines[0].starts_with(&format!(
                "2026-01-02 03:04:05.006 | udp | {earlier_coap} | tx | 49152 | 5683 | [CoAP] t:CON c:POST i:1234"
            )),
            "{}",
            lines[0]
        );
        assert!(!lines[0].contains("c:GET"), "{}", lines[0]);
    }

    #[test]
    fn pipe_delimited_capture_does_not_fallback_to_metadata_for_invalid_payload() {
        let mut parser = HexCoapParser::new();
        let line = "meta | 40011234b3666f6f03626172 | deadbeef\n";
        assert_eq!(parser.feed(line.as_bytes()), vec![line.trim_end()]);
    }

    #[test]
    fn responses_content_formats_and_block_options_are_readable() {
        let mut parser = HexCoapParser::new();
        let response = parser.feed(b"60451234ff6f6b\n");
        assert!(response[0].contains("c:2.05 Content"));
        assert!(response[0].contains("data len 2"));

        let content_format = parser.feed(b"40011234c132\n");
        assert!(content_format[0].contains("Content-Format: 50 (application/json)"));

        let block = parser.feed(b"40011234d10e1a\n");
        assert!(block[0].contains("Block1: NUM=1 M=1 SZX=2 (64B block)"));
    }

    #[test]
    fn ordinary_and_invalid_hex_lines_are_unchanged() {
        let mut parser = HexCoapParser::new();
        assert_eq!(parser.feed(b"boot complete\n"), vec!["boot complete"]);
        assert_eq!(parser.feed(b"payload deadbeef\n"), vec!["payload deadbeef"]);
    }

    #[test]
    fn partial_lines_are_buffered_by_text_parser() {
        let mut parser = HexCoapParser::new();
        assert!(parser.feed(b"rx 400112").is_empty());
        let lines = parser.feed(b"34b3666f6f03626172\n");
        assert!(lines[0].contains("[CoAP] t:CON c:GET"));
    }
}
