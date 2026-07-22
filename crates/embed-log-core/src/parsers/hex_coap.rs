//! Decode CoAP packets represented as hexadecimal text within log lines.

use regex::Regex;
use serde_json::json;

use super::text::TextParser;
use super::traits::{ParsedLine, StreamParser};

/// Line-oriented parser that replaces one embedded CoAP hex payload with a
/// readable summary while retaining the complete original line as `raw`.
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
        self.feed_entries(data)
            .into_iter()
            .map(|line| line.display)
            .collect()
    }

    fn feed_entries(&mut self, data: &[u8]) -> Vec<ParsedLine> {
        self.text.feed(data).into_iter().map(decode_line).collect()
    }
}

fn decode_line(raw: String) -> ParsedLine {
    let Some(found) = find_best_coap(&raw) else {
        // This parser is explicitly configured for the source, so preserve a
        // raw companion even for lines that do not themselves contain CoAP.
        return ParsedLine {
            display: raw.clone(),
            raw: Some(raw),
            meta: Some(json!({ "parser": "hex-coap" })),
        };
    };

    let display = format!(
        "{}[COAP {}]{}",
        &raw[..found.source_start],
        found.summary,
        &raw[found.source_end..]
    );
    ParsedLine {
        display,
        raw: Some(raw),
        meta: Some(json!({
            "parser": "hex-coap",
            "coap": {
                "version": 1,
                "type": found.message_type,
                "code": found.code_text,
                "message_id": found.message_id,
                "message_id_hex": format!("{:04x}", found.message_id),
                "token_hex": found.token_hex,
                "uri": found.uri,
                "options": found.options,
                "payload_len": found.payload_len,
            }
        })),
    }
}

#[derive(Debug)]
struct FoundCoap {
    source_start: usize,
    source_end: usize,
    summary: String,
    message_type: &'static str,
    code_text: String,
    message_id: u16,
    token_hex: String,
    uri: String,
    options: Vec<serde_json::Value>,
    payload_len: usize,
    consumed: usize,
    score: usize,
}

fn find_best_coap(raw: &str) -> Option<FoundCoap> {
    // Mirrors the frontend plugin: separated hex first, then compact hex.
    let separated = Regex::new(r"[0-9A-Fa-f][0-9A-Fa-f\s:,_|.\-]{7,}").unwrap();
    let compact = Regex::new(r"\b[0-9A-Fa-f]{8,}\b").unwrap();
    let mut candidates = Vec::new();
    candidates.extend(separated.find_iter(raw));
    candidates.extend(compact.find_iter(raw));
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.len()));

    let mut best = None;
    for candidate in candidates {
        let normalized = normalize_hex(candidate.as_str(), candidate.start());
        for start in 0..normalized.len().saturating_sub(3) {
            let decoded = parse_coap(&normalized[start..]);
            let Some(mut decoded) = decoded else { continue };
            let source_start = normalized[start].1;
            let source_end = normalized[start + decoded.consumed - 1].2;
            decoded.source_start = source_start;
            decoded.source_end = source_end;
            if best
                .as_ref()
                .is_none_or(|current: &FoundCoap| decoded.score > current.score)
            {
                best = Some(decoded);
            }
        }
    }
    best
}

/// Hex bytes paired with their offsets in the original input line.
fn normalize_hex(candidate: &str, global_start: usize) -> Vec<(u8, usize, usize)> {
    let mut digits = Vec::new();
    for (offset, byte) in candidate.bytes().enumerate() {
        if byte.is_ascii_hexdigit() {
            digits.push((byte, global_start + offset));
        }
    }
    digits
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_value(pair[0].0);
            let low = hex_value(pair[1].0);
            ((high << 4) | low, pair[0].1, pair[1].1 + 1)
        })
        .collect()
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

fn parse_coap(bytes: &[(u8, usize, usize)]) -> Option<FoundCoap> {
    if bytes.len() < 4 {
        return None;
    }
    let first = bytes[0].0;
    if first >> 6 != 1 {
        return None;
    }
    let message_type = match (first >> 4) & 0x03 {
        0 => "CON",
        1 => "NON",
        2 => "ACK",
        3 => "RST",
        _ => unreachable!(),
    };
    let token_len = usize::from(first & 0x0f);
    if token_len > 8 || bytes.len() < 4 + token_len {
        return None;
    }
    let code = bytes[1].0;
    if !looks_like_coap_code(code) {
        return None;
    }
    let message_id = u16::from_be_bytes([bytes[2].0, bytes[3].0]);
    let token = &bytes[4..4 + token_len];
    let mut index = 4 + token_len;
    let mut option_number = 0u32;
    let mut options = Vec::new();
    let mut path = Vec::new();
    let mut query = Vec::new();
    let mut payload_len = 0;

    while index < bytes.len() {
        let option = bytes[index].0;
        index += 1;
        if option == 0xff {
            payload_len = bytes.len() - index;
            break;
        }
        let (delta, next) = read_extended(bytes, index, option >> 4)?;
        index = next;
        let (length, next) = read_extended(bytes, index, option & 0x0f)?;
        index = next;
        option_number += delta;
        let length = length as usize;
        if index + length > bytes.len() {
            return None;
        }
        let value = bytes[index..index + length]
            .iter()
            .map(|(byte, _, _)| *byte)
            .collect::<Vec<_>>();
        index += length;
        let value_text = option_value(option_number, &value);
        if option_number == 11 {
            path.push(value_text.clone());
        } else if option_number == 15 {
            query.push(value_text.clone());
        }
        options.push(json!({
            "number": option_number,
            "name": option_name(option_number),
            "value": value_text,
        }));
    }

    let uri = format!(
        "{}{}",
        if path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", path.join("/"))
        },
        if query.is_empty() {
            String::new()
        } else {
            format!("?{}", query.join("&"))
        },
    );
    let code_text = code_text(code);
    let summary = if is_request(code) {
        format!("{message_type} {code_text} {uri} id={message_id:04x}")
    } else {
        format!("{message_type} {code_text} {uri} id={message_id:04x}")
    };
    let score = (usize::from(is_request(code)) * 100)
        + (usize::from(uri != "/") * 40)
        + options.len().min(8)
        + token_len.min(2)
        + usize::from(payload_len > 0);

    Some(FoundCoap {
        source_start: 0,
        source_end: 0,
        summary,
        message_type,
        code_text,
        message_id,
        token_hex: token
            .iter()
            .map(|(byte, _, _)| format!("{byte:02x}"))
            .collect(),
        uri,
        options,
        payload_len,
        consumed: index,
        score,
    })
}

fn read_extended(bytes: &[(u8, usize, usize)], index: usize, nibble: u8) -> Option<(u32, usize)> {
    match nibble {
        0..=12 => Some((u32::from(nibble), index)),
        13 => Some((u32::from(bytes.get(index)?.0) + 13, index + 1)),
        14 => Some((
            (u32::from(bytes.get(index)?.0) << 8 | u32::from(bytes.get(index + 1)?.0)) + 269,
            index + 2,
        )),
        _ => None,
    }
}

fn is_request(code: u8) -> bool {
    matches!(code, 1..=4)
}

fn looks_like_coap_code(code: u8) -> bool {
    is_request(code) || code == 0 || matches!(code >> 5, 2 | 4 | 5)
}

fn code_text(code: u8) -> String {
    match code {
        1 => "GET".to_string(),
        2 => "POST".to_string(),
        3 => "PUT".to_string(),
        4 => "DELETE".to_string(),
        65 => "2.01 Created".to_string(),
        66 => "2.02 Deleted".to_string(),
        67 => "2.03 Valid".to_string(),
        68 => "2.04 Changed".to_string(),
        69 => "2.05 Content".to_string(),
        _ => format!("{}.{:02}", code >> 5, code & 0x1f),
    }
}

fn option_name(number: u32) -> &'static str {
    match number {
        3 => "Uri-Host",
        7 => "Uri-Port",
        11 => "Uri-Path",
        12 => "Content-Format",
        14 => "Max-Age",
        15 => "Uri-Query",
        17 => "Accept",
        23 => "Block2",
        27 => "Block1",
        28 => "Size2",
        60 => "Size1",
        _ => "Option",
    }
}

fn option_value(number: u32, value: &[u8]) -> String {
    match number {
        11 | 15 | 3 => String::from_utf8_lossy(value).into_owned(),
        12 | 17 => {
            let number = uint(value);
            match number {
                0 => "0 (text/plain)".to_string(),
                50 => "50 (application/json)".to_string(),
                60 => "60 (application/cbor)".to_string(),
                _ => number.to_string(),
            }
        }
        6 | 7 | 14 | 28 | 60 => uint(value).to_string(),
        _ => value.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}

fn uint(value: &[u8]) -> u32 {
    value
        .iter()
        .fold(0, |acc, byte| (acc << 8) | u32::from(*byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_embedded_separated_hex_and_retains_raw() {
        let mut parser = HexCoapParser::new();
        let lines = parser.feed_entries(b"rx: 41 01 12 34 ab b4 74 65 73 74 rssi=-62\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].raw.as_deref(),
            Some("rx: 41 01 12 34 ab b4 74 65 73 74 rssi=-62")
        );
        assert!(lines[0].display.contains("[COAP CON GET /test id=1234]"));
        assert!(lines[0].display.ends_with(" rssi=-62"));
        assert_eq!(lines[0].meta.as_ref().unwrap()["coap"]["uri"], "/test");
    }

    #[test]
    fn accepts_compact_hex_after_arbitrary_prefix() {
        let mut parser = HexCoapParser::new();
        let lines = parser.feed_entries(b"uart rx=41011234abb474657374 end\n");
        assert!(lines[0].display.contains("GET /test"));
        assert_eq!(
            lines[0].raw.as_deref(),
            Some("uart rx=41011234abb474657374 end")
        );
    }

    #[test]
    fn leaves_invalid_hex_unchanged() {
        let mut parser = HexCoapParser::new();
        let lines = parser.feed_entries(b"counter=deadbeefcafebabe\n");
        assert_eq!(lines[0].display, "counter=deadbeefcafebabe");
        assert_eq!(lines[0].raw.as_deref(), Some("counter=deadbeefcafebabe"));
        assert_eq!(lines[0].meta.as_ref().unwrap()["parser"], "hex-coap");
    }
}
