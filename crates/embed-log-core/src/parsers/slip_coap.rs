use super::traits::StreamParser;

const SLIP_END: u8 = 0xC0;
const SLIP_ESC: u8 = 0xDB;
const SLIP_ESC_END: u8 = 0xDC;
const SLIP_ESC_ESC: u8 = 0xDD;

/// SLIP-framed UDP/CoAP parser, for device-to-device UART links.
///
/// Mirrors a legacy Python UART CoAP sniffer: each SLIP frame carries a raw
/// UDP datagram whose payload is a CoAP message.
pub struct SlipCoapParser {
    buf: Vec<u8>,
    escaping: bool,
}

impl SlipCoapParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            escaping: false,
        }
    }
}

impl Default for SlipCoapParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamParser for SlipCoapParser {
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in data {
            if byte == SLIP_END {
                if !self.buf.is_empty() {
                    lines.push(format_frame(&self.buf));
                    self.buf.clear();
                }
                self.escaping = false;
                continue;
            }
            if self.escaping {
                match byte {
                    SLIP_ESC_END => self.buf.push(SLIP_END),
                    SLIP_ESC_ESC => self.buf.push(SLIP_ESC),
                    other => self.buf.push(other),
                }
                self.escaping = false;
            } else if byte == SLIP_ESC {
                self.escaping = true;
            } else {
                self.buf.push(byte);
            }
        }
        lines
    }
}

fn format_frame(frame: &[u8]) -> String {
    // Unlike the legacy sniffer (which silently drops short frames), surface
    // them: a dropped log line reads as a bug when watching a live viewer.
    if frame.len() < 8 {
        return format!(
            "[slip frame too short for UDP header: {} byte(s)]",
            frame.len()
        );
    }
    let src_port = u16::from_be_bytes([frame[0], frame[1]]);
    let dst_port = u16::from_be_bytes([frame[2], frame[3]]);
    let payload = &frame[8..];

    // Ports 5683/5684 are the CoAP/CoAP-DTLS defaults; label direction the
    // same way the legacy sniffer does (I = inside device, O = outside device).
    let direction = if src_port == 5683 || dst_port == 5684 {
        format!("I:{src_port:>5} --> O:{dst_port:>5}")
    } else if src_port == 5684 || dst_port == 5683 {
        format!("O:{src_port:>5} --> I:{dst_port:>5}")
    } else {
        format!("S:{src_port:>5} --> D:{dst_port:>5}")
    };

    match coap::parse(payload) {
        Some(details) => format!("[{direction}]{details}"),
        None => format!(
            "[{direction}] [COAP_DECODE_ERR] payload_hex:{}",
            hex_compact(payload)
        ),
    }
}

fn hex_compact(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

mod coap {
    struct Option_ {
        number: u32,
        value: Vec<u8>,
    }

    fn read_extended(bytes: &[u8], index: usize, nibble: u8) -> Option<(u32, usize)> {
        if nibble < 13 {
            return Some((u32::from(nibble), index));
        }
        if nibble == 13 {
            let b = *bytes.get(index)?;
            return Some((u32::from(b) + 13, index + 1));
        }
        if nibble == 14 {
            let hi = u32::from(*bytes.get(index)?);
            let lo = u32::from(*bytes.get(index + 1)?);
            return Some(((hi << 8 | lo) + 269, index + 2));
        }
        None // nibble 15 is reserved
    }

    /// Parse a CoAP message, formatted like the legacy sniffer's `parse_coap_payload`.
    pub fn parse(bytes: &[u8]) -> Option<String> {
        if bytes.len() < 4 {
            return None;
        }
        let first = bytes[0];
        if first >> 6 != 1 {
            return None; // unsupported CoAP version
        }
        let mtype = (first >> 4) & 0x03;
        let token_len = usize::from(first & 0x0f);
        if token_len > 8 || bytes.len() < 4 + token_len {
            return None;
        }

        let code = bytes[1];
        let mid = u16::from_be_bytes([bytes[2], bytes[3]]);
        let mut index = 4 + token_len;
        let token = &bytes[4..index];

        let mut option_number: u32 = 0;
        let mut options = Vec::new();
        let mut payload_len = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            index += 1;
            if byte == 0xff {
                payload_len = bytes.len() - index;
                break;
            }
            let (delta, next) = read_extended(bytes, index, byte >> 4)?;
            index = next;
            let (length, next) = read_extended(bytes, index, byte & 0x0f)?;
            index = next;
            option_number += delta;
            let length = length as usize;
            if index + length > bytes.len() {
                return None;
            }
            options.push(Option_ {
                number: option_number,
                value: bytes[index..index + length].to_vec(),
            });
            index += length;
        }

        let mtype_str = match mtype {
            0 => "CON",
            1 => "NON",
            2 => "ACK",
            3 => "RST",
            _ => "UNKNOWN",
        };
        let is_request = (code >> 5) == 0 && code != 0;
        let code_str = code_text(code, is_request);
        let opt_str = if options.is_empty() {
            "[]".to_string()
        } else {
            let parts: Vec<String> = options
                .iter()
                .map(|o| {
                    format!(
                        "{}: {}",
                        option_name(o.number),
                        option_value_text(o.number, &o.value)
                    )
                })
                .collect();
            format!("[{}]", parts.join(", "))
        };

        Some(format!(
            " t:{mtype_str:<3} c:{code_str:<5} i:{mid:04x} {{{:0<16}}} {opt_str} :: data:{payload_len}",
            hex_compact(token),
        ))
    }

    fn hex_compact(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn uint_from_bytes(bytes: &[u8]) -> u32 {
        bytes.iter().fold(0u32, |acc, &b| (acc << 8) | u32::from(b))
    }

    fn code_text(code: u8, is_request: bool) -> String {
        if is_request {
            match code {
                1 => "GET".to_string(),
                2 => "POST".to_string(),
                3 => "PUT".to_string(),
                4 => "DELETE".to_string(),
                5 => "FETCH".to_string(),
                6 => "PATCH".to_string(),
                7 => "IPATCH".to_string(),
                _ => format!("{}.{:02}", code >> 5, code & 0x1f),
            }
        } else {
            format!("{}.{:02}", code >> 5, code & 0x1f)
        }
    }

    fn option_name(number: u32) -> &'static str {
        match number {
            1 => "If-Match",
            3 => "Uri-Host",
            4 => "ETag",
            5 => "If-None-Match",
            6 => "Observe",
            7 => "Uri-Port",
            8 => "Location-Path",
            9 => "OSCORE",
            11 => "Uri-Path",
            12 => "Content-Format",
            14 => "Max-Age",
            15 => "Uri-Query",
            17 => "Accept",
            20 => "Location-Query",
            23 => "Block2",
            27 => "Block1",
            28 => "Size2",
            35 => "Proxy-Uri",
            39 => "Proxy-Scheme",
            60 => "Size1",
            252 => "Echo",
            258 => "No-Response",
            292 => "Request-Tag",
            _ => "Option",
        }
    }

    /// String-valued options (RFC 7252): everything else is uint or opaque,
    /// both rendered as plain hex — same basic split the legacy sniffer's
    /// `to_hex()` makes for aiocoap's decoded option values.
    fn is_string_option(number: u32) -> bool {
        matches!(number, 3 | 8 | 11 | 15 | 20 | 35 | 39)
    }

    /// uint-valued options: rendered as bare hex (no `0x` prefix), matching
    /// `hex(val)[2:]` in the legacy sniffer.
    fn is_uint_option(number: u32) -> bool {
        matches!(number, 6 | 7 | 12 | 14 | 17 | 23 | 27 | 28 | 60 | 258)
    }

    fn option_value_text(number: u32, value: &[u8]) -> String {
        if is_string_option(number) {
            String::from_utf8_lossy(value).into_owned()
        } else if is_uint_option(number) {
            format!("{:x}", uint_from_bytes(value))
        } else {
            hex_compact(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a SLIP-framed UDP/CoAP GET request for "/test", src=5683 dst=40000.
    fn sample_frame() -> Vec<u8> {
        let coap: &[u8] = &[
            0x41, 0x01, 0x12, 0x34, // ver1 CON GET, token len 1, mid 0x1234
            0xab, // token
            0xb4, b't', b'e', b's', b't', // Uri-Path option, delta 11, len 4
        ];
        let mut udp = Vec::new();
        udp.extend_from_slice(&5683u16.to_be_bytes());
        udp.extend_from_slice(&40000u16.to_be_bytes());
        udp.extend_from_slice(&(8 + coap.len() as u16).to_be_bytes());
        udp.extend_from_slice(&[0, 0]); // checksum, unused
        udp.extend_from_slice(coap);

        let mut framed = vec![SLIP_END];
        framed.extend_from_slice(&udp);
        framed.push(SLIP_END);
        framed
    }

    #[test]
    fn decodes_coap_get_request() {
        let mut parser = SlipCoapParser::new();
        let lines = parser.feed(&sample_frame());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("I: 5683 --> O:40000"));
        assert!(lines[0].contains("t:CON"));
        assert!(lines[0].contains("c:GET"));
        assert!(lines[0].contains("i:1234"));
        assert!(lines[0].contains("Uri-Path: test"));
        assert!(lines[0].contains("data:0"));
    }

    #[test]
    fn splits_multiple_frames_from_one_chunk() {
        let mut parser = SlipCoapParser::new();
        let mut chunk = sample_frame();
        chunk.extend_from_slice(&sample_frame());
        let lines = parser.feed(&chunk);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn reassembles_frame_split_across_feed_calls() {
        let mut parser = SlipCoapParser::new();
        let frame = sample_frame();
        let (a, b) = frame.split_at(frame.len() / 2);
        assert!(parser.feed(a).is_empty());
        let lines = parser.feed(b);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Uri-Path: test"));
    }

    #[test]
    fn unescapes_slip_escape_sequences() {
        let mut parser = SlipCoapParser::new();
        // A byte stream containing an escaped END (0xC0) inside the frame.
        let framed = [SLIP_END, 0xAA, SLIP_ESC, SLIP_ESC_END, 0xBB, SLIP_END];
        parser.feed(&framed);
        // Frame is too short for a UDP header (2 bytes) but should have
        // decoded to [0xAA, 0xC0, 0xBB], i.e. escaping worked.
        let mut parser2 = SlipCoapParser::new();
        let lines = parser2.feed(&framed);
        assert_eq!(
            lines,
            vec!["[slip frame too short for UDP header: 3 byte(s)]"]
        );
    }

    #[test]
    fn reports_frame_too_short_for_udp_header() {
        let mut parser = SlipCoapParser::new();
        let framed = [SLIP_END, 1, 2, 3, SLIP_END];
        let lines = parser.feed(&framed);
        assert_eq!(
            lines,
            vec!["[slip frame too short for UDP header: 3 byte(s)]"]
        );
    }

    #[test]
    fn reports_coap_decode_error_for_malformed_payload() {
        let mut parser = SlipCoapParser::new();
        let mut udp = Vec::new();
        udp.extend_from_slice(&5683u16.to_be_bytes());
        udp.extend_from_slice(&40000u16.to_be_bytes());
        udp.extend_from_slice(&[0, 0, 0, 0]); // length/checksum, unused
        udp.extend_from_slice(&[0x00, 0x00]); // version 0 -> invalid CoAP

        let mut framed = vec![SLIP_END];
        framed.extend_from_slice(&udp);
        framed.push(SLIP_END);

        let lines = parser.feed(&framed);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("COAP_DECODE_ERR"));
        assert!(lines[0].contains("payload_hex:0000"));
    }
}
