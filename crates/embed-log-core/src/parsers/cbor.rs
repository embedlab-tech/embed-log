use super::traits::StreamParser;

/// CBOR datagram parser.
///
/// Each feed call is treated as one complete CBOR datagram (typically one UDP
/// packet).  The CBOR map is decoded and rendered as `key=value` text pairs.
pub struct CborDatagramParser;

impl CborDatagramParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CborDatagramParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamParser for CborDatagramParser {
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        let mut cursor = std::io::Cursor::new(data);
        let decoded: ciborium::Value = match ciborium::from_reader(&mut cursor) {
            Ok(v) => v,
            Err(e) => {
                return vec![format!("[cbor decode error: {e}]")];
            }
        };
        if cursor.position() != data.len() as u64 {
            return vec![format!(
                "[cbor decode error: trailing {} byte(s)]",
                data.len() as u64 - cursor.position()
            )];
        }

        match decoded {
            ciborium::Value::Map(map) => {
                let pairs: Vec<String> = map
                    .iter()
                    .filter_map(|(k, v)| {
                        let key = cbor_value_to_string(k);
                        let val = cbor_value_to_string(v);
                        if key.is_empty() {
                            None
                        } else {
                            Some(format!("{key}={val}"))
                        }
                    })
                    .collect();
                if pairs.is_empty() {
                    vec!["(empty CBOR map)".to_string()]
                } else {
                    vec![pairs.join("  ")]
                }
            }
            other => vec![format!("(non-map CBOR: {})", cbor_value_to_string(&other))],
        }
    }
}

fn cbor_value_to_string(val: &ciborium::Value) -> String {
    match val {
        ciborium::Value::Text(s) => s.clone(),
        ciborium::Value::Integer(i) => {
            // ciborium Integer is an i128.
            format!("{}", i128::from(*i))
        }
        ciborium::Value::Float(f) => format!("{f}"),
        ciborium::Value::Bool(b) => format!("{b}"),
        ciborium::Value::Null => "null".to_string(),
        ciborium::Value::Bytes(b) => hex_preview(b),
        ciborium::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(cbor_value_to_string).collect();
            format!("[{}]", items.join(", "))
        }
        ciborium::Value::Map(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}:{}", cbor_value_to_string(k), cbor_value_to_string(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        _ => "?".to_string(),
    }
}

fn hex_preview(data: &[u8]) -> String {
    let preview: Vec<String> = data.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let suffix = if data.len() > 8 { "…" } else { "" };
    format!("hex:{}{suffix}", preview.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_simple_map() {
        let mut parser = CborDatagramParser::new();

        // Build a CBOR map: {"temp": 25, "unit": "C"}
        let value = ciborium::Value::Map(vec![
            (
                ciborium::Value::Text("temp".to_string()),
                ciborium::Value::Integer(25.into()),
            ),
            (
                ciborium::Value::Text("unit".to_string()),
                ciborium::Value::Text("C".to_string()),
            ),
        ]);

        let mut encoded = Vec::new();
        ciborium::into_writer(&value, &mut encoded).unwrap();

        let lines = parser.feed(&encoded);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("temp=25"));
        assert!(lines[0].contains("unit=C"));
    }

    #[test]
    fn decode_error() {
        let mut parser = CborDatagramParser::new();
        let lines = parser.feed(&[0xff, 0xff, 0xff]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("cbor decode error"));
    }

    #[test]
    fn decode_empty_map() {
        let mut parser = CborDatagramParser::new();
        let value = ciborium::Value::Map(vec![]);
        let mut encoded = Vec::new();
        ciborium::into_writer(&value, &mut encoded).unwrap();
        let lines = parser.feed(&encoded);
        assert_eq!(lines, vec!["(empty CBOR map)"]);
    }

    #[test]
    fn reject_trailing_bytes() {
        let mut parser = CborDatagramParser::new();
        let value = ciborium::Value::Map(vec![]);
        let mut encoded = Vec::new();
        ciborium::into_writer(&value, &mut encoded).unwrap();
        encoded.push(0);

        let lines = parser.feed(&encoded);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("trailing"));
    }
}
