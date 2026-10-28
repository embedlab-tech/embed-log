//! Synthetic wire regressions without hardware or external fixtures.
use super::*;
use std::path::PathBuf;
use std::time::Duration;

struct TestDb(PathBuf);
impl TestDb {
    fn new(mappings: &[(u64, &str)]) -> Self {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("embed-log-regression-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mappings: serde_json::Map<String, serde_json::Value> = mappings
            .iter()
            .map(|(p, s)| (p.to_string(), serde_json::json!(s)))
            .collect();
        let db = serde_json::json!({"version":3,"target":{"bits":32,"little_endianness":true},"arch":"arm","log_subsys":{"log_instances":{"1":{"name":"app"}}},"string_mappings":mappings});
        let path = dir.join("dictionary.json");
        std::fs::write(&path, serde_json::to_vec(&db).unwrap()).unwrap();
        Self(path)
    }
    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
    fn load(&self) -> Database {
        Database::load(self.path()).unwrap()
    }
}
impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}
fn package(words: &[u32], strings: &[(u8, &str)]) -> Vec<u8> {
    let mut bytes = vec![(2 + words.len()) as u8, strings.len() as u8, 0, 0];
    bytes.extend(0x1000u32.to_le_bytes());
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    for (idx, s) in strings {
        bytes.push(*idx);
        bytes.extend(s.as_bytes());
        bytes.push(0);
    }
    bytes
}
fn record(pkg: &[u8], data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 0x30];
    bytes.extend((pkg.len() as u16).to_le_bytes());
    bytes.extend((data.len() as u16).to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(42u32.to_le_bytes());
    bytes.extend(pkg);
    bytes.extend(data);
    bytes
}
fn assert_both_transports(db: &TestDb, pkg: &[u8], body: &str) {
    let wire = record(pkg, &[]);
    let expected = vec![format!("[00000042] <inf> app: {body}")];
    for size in 1..=wire.len() {
        let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
        let got: Vec<_> = wire.chunks(size).flat_map(|c| parser.feed(c)).collect();
        assert_eq!(got, expected, "BIN chunk {size}");
    }
    let hex = format!(
        "##ZLOGV1##{}\n",
        wire.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    for size in 1..=hex.len() {
        let mut parser = ZephyrDictParser::new(db.path());
        let got: Vec<_> = hex
            .as_bytes()
            .chunks(size)
            .flat_map(|c| parser.feed(c))
            .collect();
        assert_eq!(got, expected, "HEX chunk {size}");
    }
}
#[test]
fn stale_incomplete_record_cannot_hold_later_known_records_hostage() {
    let db = TestDb::new(&[(0x1000, "current firmware")]);
    let good = record(&package(&[], &[]), &[]);
    let mut stale = good.clone();
    stale[2..4].copy_from_slice(&65535u16.to_le_bytes());
    let mut wire = stale;
    wire.extend(&good);
    for size in 1..=wire.len() {
        let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
        let got: Vec<_> = wire.chunks(size).flat_map(|c| parser.feed(c)).collect();
        assert!(
            got.iter().any(|s| s.contains("app: current firmware")),
            "chunk {size}: {got:?}"
        );
    }
}
#[test]
fn truncated_record_releases_plain_text_on_hard_uart_idle() {
    let db = TestDb::new(&[(0x1000, "current firmware")]);
    let good = record(&package(&[], &[]), &[]);
    let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
    assert_eq!(parser.feed(&good).len(), 1);
    let mut truncated = good.clone();
    truncated[4..6].copy_from_slice(&65535u16.to_le_bytes());
    truncated.extend(b"*** Booting firmware ***\r\nBOOT COMPLETE\r\n");
    parser.feed(&truncated);
    for millis in (1000..5000).step_by(250) {
        assert!(parser.on_idle(Duration::from_millis(millis)).is_empty());
    }
    let got = parser.on_idle(Duration::from_secs(5));
    assert!(got.iter().any(|s| s.contains("BOOT COMPLETE")), "{got:?}");
    assert_eq!(got.iter().filter(|s| s.contains("abandoned")).count(), 1);
    assert!(got
        .iter()
        .any(|s| s.contains("valid delayed data may be lost")));
    assert_eq!(
        got.iter().filter(|s| s.contains("BOOT COMPLETE")).count(),
        1
    );
    for secs in 5..=10 {
        assert!(parser.on_idle(Duration::from_secs(secs)).is_empty());
    }
    assert!(parser
        .feed(&good)
        .iter()
        .any(|s| s.contains("current firmware")));
    assert!(parser.on_idle(Duration::from_secs(1)).is_empty());
}
#[test]
fn foreign_firmware_format_is_rejected_without_waiting_for_hexdump() {
    let db = TestDb::new(&[(0x1000, "current firmware")]);
    let mut stale = record(&package(&[], &[]), &[]);
    stale[18..22].copy_from_slice(&0xdeadbeefu32.to_le_bytes());
    stale[4..6].copy_from_slice(&65535u16.to_le_bytes());
    stale.extend(b"BOOT COMPLETE\r\n");
    let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
    let mut got = parser.feed(&stale);
    got.extend(parser.on_idle(Duration::from_secs(1)));
    assert!(got.iter().any(|s| s.contains("BOOT COMPLETE")), "{got:?}");
}
#[test]
fn valid_hexdump_containing_an_entire_record_is_not_resynchronized() {
    let db = TestDb::new(&[(0x1000, "outer")]);
    let inner = record(&package(&[], &[]), &[]);
    let outer = record(&package(&[], &[]), &inner);
    for acquired in [false, true] {
        let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
        if acquired {
            assert_eq!(
                parser
                    .feed(&inner)
                    .iter()
                    .filter(|l| l.contains("app: outer"))
                    .count(),
                1
            );
        }
        let got: Vec<_> = outer.chunks(1).flat_map(|c| parser.feed(c)).collect();
        assert_eq!(
            got.iter().filter(|l| l.contains("app: outer")).count(),
            1,
            "{got:?}"
        );
        assert!(!got.iter().any(|l| l.contains("zephyr-dict:")), "{got:?}");
    }
}
#[test]
fn dynamic_precision_consumes_int_and_preserves_following_arguments() {
    let db = TestDb::new(&[
        (0x1000, "reason=%.*s status=%d next=%s"),
        (0x2000, "abcdef"),
        (0x3000, "done"),
    ]);
    assert_both_transports(
        &db,
        &package(&[3, 0x2000, 77, 0x3000], &[]),
        "reason=abc status=77 next=done",
    );
    assert_both_transports(
        &db,
        &package(&[3, 0xdeadbeef, 77, 0x3000], &[(3, "abcdef")]),
        "reason=abc status=77 next=done",
    );
}
#[test]
fn dynamic_width_and_precision_follow_c_signed_semantics() {
    let db = TestDb::new(&[(0x1000, "[%*.*s] next=%d"), (0x2000, "abcdef")]);
    for (width, precision, body) in [
        (6, 3, "[   abc] next=9"),
        (-6, 3, "[abc   ] next=9"),
        (6, -1, "[abcdef] next=9"),
        (6, 0, "[      ] next=9"),
    ] {
        assert_both_transports(
            &db,
            &package(&[width as u32, precision as u32, 0x2000, 9], &[]),
            body,
        );
    }
}
#[test]
fn arm_mixed_u32_u64_double_and_pointer_keep_alignment() {
    let db = TestDb::new(&[(0x1000, "%u %llu %.2f %p")]);
    let wide = 0x123456789abcdef0u64;
    let float = 1.25f64.to_bits();
    assert_both_transports(
        &db,
        &package(
            &[
                7,
                0,
                wide as u32,
                (wide >> 32) as u32,
                float as u32,
                (float >> 32) as u32,
                0x2000,
            ],
            &[],
        ),
        "7 1311768467463790320 1.25 0x2000",
    );
}
#[test]
fn null_and_empty_inline_strings_are_distinct_from_unknown_pointers() {
    let db = TestDb::new(&[(0x1000, "[%s] [%s]")]);
    assert_both_transports(&db, &package(&[0, 0xdeadbeef], &[(3, "")]), "[(null)] []");
}
#[test]
fn truncated_dynamic_argument_is_not_silently_rendered_as_a_format() {
    let db = TestDb::new(&[(0x1000, "%.*s")]);
    assert!(decode_package(&package(&[3], &[]), &db.load()).is_err());
}
#[test]
fn literal_percent_does_not_consume_an_argument() {
    let db = TestDb::new(&[(0x1000, "100%% [%*d]")]);
    assert_both_transports(&db, &package(&[4, 7], &[]), "100% [   7]");
}
#[test]
fn empty_string_at_mapped_terminator_is_resolved_but_unknown_pointer_is_not() {
    let db = TestDb::new(&[(0x1000, "Identity%s next=%s"), (0x2000, "abc")]);
    assert_both_transports(
        &db,
        &package(&[0x2003, 0xdeadbeef], &[]),
        "Identity next=<string@0xdeadbeef>",
    );
}
#[test]
fn empty_string_inside_binary_section_is_resolved() {
    use base64::Engine;
    let db = TestDb::new(&[(0x1000, "Activate %scomplete")]);
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&db.0).unwrap()).unwrap();
    json["sections"] = serde_json::json!({"rodata":{"start":0x2000,"size":5,"data_b64":base64::engine::general_purpose::STANDARD.encode([0x2a,0x86,0,0xce,0x3d])}});
    std::fs::write(&db.0, serde_json::to_vec(&json).unwrap()).unwrap();
    assert_both_transports(&db, &package(&[0x2002], &[]), "Activate complete");
}
#[test]
fn inline_utf8_strings_are_not_decoded_as_latin1() {
    let db = TestDb::new(&[(0x1000, "%s")]);
    assert_both_transports(&db, &package(&[0xdeadbeef], &[(2, "zażółć")]), "zażółć");
}
#[test]
fn ordinary_string_arguments_are_not_rewritten_as_format_strings() {
    let db = TestDb::new(&[(0x1000, "%s"), (0x2000, "literal %p %zu")]);
    assert_both_transports(&db, &package(&[0x2000], &[]), "literal %p %zu");
}
#[test]
fn invalid_utf8_suffix_pointer_does_not_panic() {
    let db = TestDb::new(&[(0x1000, "%s"), (0x2000, "ż")]);
    assert_eq!(db.load().find_string(0x2001), None);
}
#[test]
fn unknown_format_pointer_is_not_a_successfully_decoded_record() {
    let db = TestDb::new(&[]);
    assert!(decode_package(&package(&[], &[]), &db.load()).is_err());
}
#[test]
fn unmapped_string_is_reported_in_full_even_with_precision() {
    let db = TestDb::new(&[(0x1000, "%.3s")]);
    let got = decode_package(&package(&[0xdeadbeef], &[]), &db.load()).unwrap();
    assert!(got.contains("string@0xdeadbeef"), "{got}");
}

#[test]
fn delayed_package_preserves_signed_argument_and_following_drop_marker() {
    let db = TestDb::new(&[(0x1000, "operation failed: %d")]);
    let wire = record(&package(&[(-7i32) as u32], &[]), &[]);
    let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
    assert!(parser.feed(&wire[..14]).is_empty());
    for millis in [1000, 1250] {
        assert!(parser.on_idle(Duration::from_millis(millis)).is_empty());
    }
    // Complete the package after soft idle but before the next callback.
    let mut got = parser.feed(&wire[14..]);
    got.extend(parser.feed(&[1, 69, 0]));
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].contains("app: operation failed: -7"));
    assert!(got[1].contains("69 messages dropped"));
    assert!(parser.on_idle(Duration::from_secs(5)).is_empty());
}

#[test]
fn soft_idle_preserves_every_header_package_payload_and_drop_boundary() {
    let db = TestDb::new(&[(0x1000, "outer")]);
    let good = record(&package(&[], &[]), &[]);
    // A whole decodable record inside the hexdump must never become a resync oracle.
    let mut wire = record(&package(&[], &[]), &good);
    wire.extend([1, 69, 0]);
    for acquired in [false, true] {
        for split in 1..wire.len() {
            for resume_ms in [1295, 4999] {
                let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
                if acquired {
                    assert_eq!(parser.feed(&good).len(), 1);
                }
                let mut got = parser.feed(&wire[..split]);
                for millis in (1000..resume_ms).step_by(250) {
                    got.extend(parser.on_idle(Duration::from_millis(millis)));
                }
                got.extend(parser.feed(&wire[split..]));
                let mut reference = binary::ZephyrDictBinaryParser::new(db.path());
                let expected = reference.feed(&wire);
                assert_eq!(
                    got, expected,
                    "acquired={acquired} split={split} resume={resume_ms}"
                );
                assert!(parser.on_idle(Duration::from_secs(5)).is_empty());
            }
        }
    }
}

#[test]
fn hard_idle_drains_even_short_headers_and_drop_markers_once() {
    let db = TestDb::new(&[(0x1000, "alive")]);
    let good = record(&package(&[], &[]), &[]);
    let mut wire = good.clone();
    wire.extend([1, 69, 0]);
    for split in 1..wire.len() {
        let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
        parser.feed(&good);
        parser.feed(&wire[..split]);
        parser.on_idle(Duration::from_secs(1));
        let got = parser.on_idle(Duration::from_secs(5));
        let incomplete = split != good.len();
        assert_eq!(
            got.iter().filter(|s| s.contains("abandoned")).count(),
            usize::from(incomplete),
            "split={split}: {got:?}"
        );
        assert!(
            parser.on_idle(Duration::from_secs(6)).is_empty(),
            "split={split}"
        );
        assert!(parser.feed(&good).iter().any(|s| s.contains("app: alive")));
    }
}

#[test]
fn soft_idle_flushes_only_classified_console_text_and_keeps_sync() {
    let db = TestDb::new(&[(0x1000, "alive")]);
    let good = record(&package(&[], &[]), &[]);
    let mut parser = binary::ZephyrDictBinaryParser::new(db.path());
    parser.feed(&good);
    let mut wire = b"prompt> ".to_vec();
    wire.extend(&good[..14]);
    assert!(parser.feed(&wire).is_empty());
    assert_eq!(parser.on_idle(Duration::from_secs(1)), vec!["prompt> "]);
    assert!(parser.on_idle(Duration::from_millis(1250)).is_empty());
    assert_eq!(parser.feed(&good[14..]).len(), 1);
    assert_eq!(
        parser.feed(&[1, 69, 0]),
        vec!["--- 69 messages dropped ---"]
    );
}
