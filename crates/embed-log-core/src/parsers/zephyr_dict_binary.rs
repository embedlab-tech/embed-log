//! Standard Zephyr v3 dictionary binary records, optionally mixed with console text.
//!
//! Uses the same record decoder as the HEX parser. No wire framing is added.
//! Acquisition is heuristic: initial data can be lost, and arbitrary binary
//! console output or corruption cannot be classified without ambiguity.
use super::{
    read_uint_sized, take_one_message, Database, LoadState, TakeResult, MSG_TYPE_DROPPED,
    MSG_TYPE_NORMAL,
};
use crate::parsers::traits::StreamParser;

pub struct ZephyrDictBinaryParser {
    state: LoadState,
    pending: Vec<u8>,
    synchronized: bool,
    skipped: usize,
    reported_load_error: bool,
    console_text: Vec<u8>,
}

impl ZephyrDictBinaryParser {
    pub fn new(database_path: &str) -> Self {
        Self {
            state: match Database::load(database_path) {
                Ok(db) => LoadState::Ready(db),
                Err(err) => LoadState::Failed(err),
            },
            pending: Vec::new(),
            synchronized: false,
            skipped: 0,
            reported_load_error: false,
            console_text: Vec::new(),
        }
    }

    fn consume(&mut self, lines: &mut Vec<String>) {
        let LoadState::Ready(db) = &self.state else {
            return;
        };
        let mut offset = 0;
        while offset < self.pending.len() {
            let bytes = &self.pending[offset..];
            // Console text can occur at EVERY record boundary. Keep the
            // acquired dictionary confidence across text, including ANSI/UTF-8.
            // Never scan for text inside a validated self-length-prefixed record.
            if bytes[0] != MSG_TYPE_NORMAL && bytes[0] != MSG_TYPE_DROPPED {
                if bytes[0] >= 0x20 || matches!(bytes[0], b'\t' | b'\r' | b'\n' | 0x1b) {
                    push_binary_console(&mut self.console_text, bytes[0], lines);
                } else {
                    self.skipped += 1;
                }
                offset += 1;
                continue;
            }
            // A standalone three-byte dropped-message record is ambiguous on
            // initial attachment. Acquire with a complete normal record first.
            if bytes[0] == MSG_TYPE_DROPPED && !self.synchronized {
                offset += 1;
                self.skipped += 1;
                continue;
            }
            if bytes[0] == MSG_TYPE_NORMAL {
                if bytes.len() < 2 {
                    break;
                }
                let level = if db.little_endian {
                    bytes[1] >> 4
                } else {
                    bytes[1] & 15
                };
                if level > 4 {
                    offset += 1;
                    self.skipped += 1;
                    continue;
                }
                let prefix = 6 + db.ptr_size();
                if bytes.len() < prefix {
                    break;
                }
                let pkg_len = read_uint_sized(&bytes[2..4], db.little_endian, 2).unwrap();
                let source =
                    read_uint_sized(&bytes[6..prefix], db.little_endian, db.ptr_size()).unwrap();
                let known = db.log_instances.contains_key(&source.to_string());
                // LOG_RAW uses a null source and a dictionary/package format.
                // Accept it after acquisition, not as an initial noise oracle.
                let raw = self.synchronized && level == 0 && source == 0;
                if pkg_len < (4 + db.ptr_size()) as u64 || (!known && !raw) {
                    offset += 1;
                    self.skipped += 1;
                    continue;
                }
            }
            match take_one_message(bytes, db) {
                TakeResult::NeedMore => break,
                TakeResult::Consumed { len, mut output } => {
                    flush_binary_console(&mut self.console_text, lines);
                    if self.skipped != 0 {
                        lines.push(format!(
                            "[zephyr-dict: skipped {} bytes while acquiring stream]",
                            self.skipped
                        ));
                        self.skipped = 0;
                    }
                    self.synchronized = true;
                    offset += len;
                    lines.append(&mut output);
                }
                TakeResult::Stop => {
                    if self.synchronized {
                        lines.push("[zephyr-dict: invalid record; synchronization lost]".into());
                    }
                    self.synchronized = false;
                    offset += 1;
                    self.skipped += 1;
                }
            }
        }
        self.pending.drain(..offset);
    }
}

impl StreamParser for ZephyrDictBinaryParser {
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        if let LoadState::Failed(err) = &self.state {
            if self.reported_load_error {
                return Vec::new();
            }
            self.reported_load_error = true;
            return vec![format!(
                "[zephyr-dict: database not loaded ({err}); dropping incoming bytes]"
            )];
        }
        let mut lines = Vec::new();
        for chunk in data.chunks(4096) {
            self.pending.extend_from_slice(chunk);
            self.consume(&mut lines);
            // Wire length fields are u16; even a maximum-sized record is less
            // than 132 KiB. Bound memory independently of UART read sizes.
            if self.pending.len() > 256 * 1024 {
                self.pending.clear();
                self.synchronized = false;
                lines.push("[zephyr-dict: buffer limit exceeded; synchronization lost]".into());
            }
        }
        lines
    }
}

fn flush_binary_console(text: &mut Vec<u8>, lines: &mut Vec<String>) {
    if !text.is_empty() {
        let line = String::from_utf8_lossy(text);
        let line = line.trim_end_matches('\r');
        if !line.is_empty() {
            lines.push(line.to_owned());
        }
        text.clear();
    }
}

fn push_binary_console(text: &mut Vec<u8>, byte: u8, lines: &mut Vec<String>) {
    if byte == b'\n' {
        flush_binary_console(text, lines);
    } else {
        text.push(byte);
        if text.len() >= 64 * 1024 {
            flush_binary_console(text, lines);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ZephyrDictParser;
    use super::*;
    use std::io::Write;
    /// Build a minimal version-3 database.json with the given string
    /// mappings (address -> string) and one log source named "app".
    fn write_test_db(dir: &std::path::Path, string_mappings: &[(u64, &str)]) -> String {
        let mappings: serde_json::Map<String, serde_json::Value> = string_mappings
            .iter()
            .map(|(addr, s)| (addr.to_string(), serde_json::json!(s)))
            .collect();
        let db = serde_json::json!({
            "version": 3,
            "target": { "bits": 32, "little_endianness": true },
            "arch": "arm",
            "kconfigs": {},
            "log_subsys": { "log_instances": { "1": { "source_id": 1, "name": "app", "level": 4, "addr": 0 } } },
            "string_mappings": mappings,
            "sections": {},
        });
        let path = dir.join("database.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(serde_json::to_string(&db).unwrap().as_bytes())
            .unwrap();
        path.display().to_string()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("embed-log-zephyr-dict-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Encode one v3 NORMAL message: fmt string + args resolved via static
    /// string_mappings (no inline string table needed).
    fn encode_normal_message(
        level: u8,
        source_id: u32,
        timestamp: u32,
        fmt_str_ptr: u32,
        arg_bytes: &[u8],
        extra_data: &[u8],
    ) -> Vec<u8> {
        // domain=0, level in bits 4..7 (little-endian layout)
        let domain_lvl = (level << 4) & 0xF0;

        // package = [end_of_args_units, num_packed(0), num_ro(0), num_rw(0),
        //            fmt_str_ptr(ptr_size), ...args...]
        let hdr_len = 4 + 4; // 32-bit ptr
        let arg_list_len = arg_bytes.len();
        let end_of_args_abs = hdr_len + arg_list_len;
        assert_eq!(end_of_args_abs % 4, 0, "test arg list must be int-aligned");
        let end_of_args_units = (end_of_args_abs / 4) as u8;

        let mut package = vec![
            end_of_args_units,
            0, // num_packed_strings
            0, // num_ro_str_indexes
            0, // num_rw_str_indexes
        ];
        package.extend_from_slice(&fmt_str_ptr.to_le_bytes());
        package.extend_from_slice(arg_bytes);
        // no inline string table needed since strings are statically resolved

        let pkg_len = package.len() as u16;
        let data_len = extra_data.len() as u16;

        let mut msg = Vec::new();
        msg.push(MSG_TYPE_NORMAL);
        msg.push(domain_lvl);
        msg.extend_from_slice(&pkg_len.to_le_bytes());
        msg.extend_from_slice(&data_len.to_le_bytes());
        msg.extend_from_slice(&source_id.to_le_bytes()); // 32-bit source ptr
        msg.extend_from_slice(&timestamp.to_le_bytes()); // 32-bit timestamp
        msg.extend_from_slice(&package);
        msg.extend_from_slice(extra_data);
        msg
    }

    fn feed_hex(parser: &mut ZephyrDictParser, message: &[u8]) -> Vec<String> {
        let hex: String = message.iter().map(|byte| format!("{byte:02x}")).collect();
        parser.feed(format!("##ZLOGV1##{hex}\n").as_bytes())
    }

    #[test]
    fn factory_selects_binary_and_hex_without_auto_detection() {
        let dir = temp_dir("dictionary-factory");
        let db = write_test_db(&dir, &[(0x1000, "factory ok\n")]);
        let binary = encode_normal_message(3, 1, 12, 0x1000, &[], &[]);
        let hex = format!(
            "##ZLOGV1##{}\n",
            binary
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        for (name, bytes) in [
            ("zephyr-dict", binary.as_slice()),
            ("zephyr-dict-hex", hex.as_bytes()),
        ] {
            let config = crate::config::models::ParserConfig {
                parser_type: name.into(),
                database: Some(db.clone()),
            };
            let mut parser = crate::parsers::create_parser(&config);
            assert!(
                parser
                    .feed(bytes)
                    .iter()
                    .any(|line| line.contains("app: factory ok")),
                "{name}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn binary_console_preserves_text_and_raw_records_for_every_chunk() {
        let dir = temp_dir("bin-console-chunks");
        let db = write_test_db(&dir, &[(0x1000, "boot ok\n"), (0x2000, "raw ok\n")]);
        let normal = encode_normal_message(3, 1, 12, 0x1000, &[], &(0..=255).collect::<Vec<u8>>());
        let raw = encode_normal_message(0, 0, 13, 0x2000, &[], &[]);
        let mut hex_wire = normal.clone();
        hex_wire.extend(&raw);
        hex_wire.extend([1, 3, 0]);
        let mut hex = ZephyrDictParser::new(&db);
        let reference = hex.feed(
            format!(
                "##ZLOGV1##{}\n",
                hex_wire
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            )
            .as_bytes(),
        );
        let mut wire = b"deadbeef\r\n\x1b[1;1f012345\r\n".to_vec();
        wire.extend(&normal);
        wire.extend(b"device> ");
        wire.extend(&raw);
        wire.extend(b"za\xc5\xbc\xc3\xb3\xc5\x82\xc4\x87\r\n");
        wire.extend([1, 3, 0]);
        let split = reference.len() - 2; // raw log and dropped-message notification
        let mut expected = vec!["deadbeef".to_string(), "\x1b[1;1f012345".into()];
        expected.extend_from_slice(&reference[..split]);
        expected.push("device> ".into());
        expected.push(reference[split].clone());
        expected.push("zażółć".into());
        expected.push(reference[split + 1].clone());
        for size in 1..=wire.len() {
            let mut parser = ZephyrDictBinaryParser::new(&db);
            let got: Vec<_> = wire.chunks(size).flat_map(|c| parser.feed(c)).collect();
            assert_eq!(got, expected, "chunk {size}");
        }
    }

    #[test]
    fn binary_console_reacquires_after_mid_record_attachment() {
        let dir = temp_dir("bin-console-attach");
        let db = write_test_db(&dir, &[(0x1000, "recovered\n")]);
        let normal = encode_normal_message(3, 1, 12, 0x1000, &[], &[]);
        for offset in 1..normal.len() {
            let mut wire = normal[offset..].to_vec();
            wire.extend(b"help\r\n");
            wire.extend(&normal);
            wire.extend(b"deadbeef\n");
            let mut parser = ZephyrDictBinaryParser::new(&db);
            let got: Vec<_> = wire.chunks(1).flat_map(|c| parser.feed(c)).collect();
            assert!(
                got.iter().any(|l| l.contains("<inf> app: recovered")),
                "offset {offset}: {got:?}"
            );
            assert!(got.iter().any(|l| l == "deadbeef"));
        }
    }

    #[test]
    fn binary_matches_hex_for_every_chunk_size() {
        let dir = temp_dir("binary-equivalence");
        let db = write_test_db(&dir, &[(0x1000, "count=%d\n"), (0x2000, "payload\n")]);
        let mut bytes = encode_normal_message(3, 1, 42, 0x1000, &(-7i32).to_le_bytes(), &[]);
        bytes.extend([MSG_TYPE_DROPPED, 5, 0]);
        bytes.extend(encode_normal_message(
            4,
            1,
            43,
            0x2000,
            &[],
            &[0, 10, 13, 27, 255],
        ));
        let expected = feed_hex(&mut ZephyrDictParser::new(&db), &bytes);
        assert!(expected.iter().any(|line| line.contains("count=-7")));
        assert!(expected
            .iter()
            .any(|line| line.contains("5 messages dropped")));
        for size in 1..=bytes.len() {
            let mut parser = ZephyrDictBinaryParser::new(&db);
            let got: Vec<_> = bytes
                .chunks(size)
                .flat_map(|part| parser.feed(part))
                .collect();
            assert_eq!(got, expected, "chunk size {size}");
            assert!(parser.pending.is_empty());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn binary_skips_boot_noise_without_inventing_drop_counts() {
        let dir = temp_dir("binary-boot");
        let db = write_test_db(&dir, &[(0x1000, "boot")]);
        let mut parser = ZephyrDictBinaryParser::new(&db);
        let mut bytes = vec![1, 0x23, 0x45, 0xff, b'\n'];
        bytes.extend(encode_normal_message(3, 1, 1, 0x1000, &[], &[]));
        let lines = parser.feed(&bytes);
        // In mixed mode printable boot bytes are retained as console text,
        // not counted as silently discarded binary input.
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "#E\u{fffd}");
        assert!(lines[1].contains("skipped 1 bytes"));
        assert!(lines[2].contains("app: boot"));
        assert!(!lines.iter().any(|s| s.contains("messages dropped")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn binary_reports_corruption_and_reacquires_a_known_record() {
        let dir = temp_dir("binary-corrupt");
        let db = write_test_db(&dir, &[(0x1000, "ok")]);
        let bytes = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let mut parser = ZephyrDictBinaryParser::new(&db);
        assert_eq!(parser.feed(&bytes).len(), 1);
        // A valid-looking header but an out-of-bounds cbprintf argument
        // list is detectable corruption. Arbitrary text bytes alone are not.
        let mut damaged = bytes.clone();
        damaged[14] = 0xff;
        damaged.extend(&bytes);
        let lines = parser.feed(&damaged);
        assert!(lines.iter().any(|s| s.contains("synchronization lost")));
        assert!(lines.last().unwrap().contains("app: ok"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn binary_missing_dictionary_reports_once_and_junk_is_bounded() {
        let mut failed = ZephyrDictBinaryParser::new("/nonexistent/binary-dictionary.json");
        assert_eq!(failed.feed(&[0, 0, 0]).len(), 1);
        assert!(failed.feed(&[0, 0]).is_empty());
        let dir = temp_dir("binary-bound");
        let db = write_test_db(&dir, &[(0x1000, "ok")]);
        let mut parser = ZephyrDictBinaryParser::new(&db);
        parser.feed(&vec![0xff; 1024 * 1024]);
        assert!(parser.pending.len() < 256 * 1024);
        assert!(parser.console_text.len() < 64 * 1024);
        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        assert!(parser.feed(&msg).last().unwrap().contains("app: ok"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
