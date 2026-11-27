//! Offline decoding of `.log` text that holds undecoded dictionary records.
//!
//! Binary mode (`zephyr-dict`): a line `[ts] [dict-raw:v1] <base64>` carries the
//! bytes of one record that was captured while no dictionary was available.
//! HEX mode (`zephyr-dict-hex`): records are already ASCII (`##ZLOGV1##<hex>`),
//! so the line itself is the capture. Either way the bytes are fed to the real
//! parser in file order, so decoding is the same code path as live decoding.
//! Every other line is copied unchanged.
use super::{try_create_dictionary_parser, StreamParser, RAW_LINE_PREFIX};
use crate::config::models::ParserConfig;
use base64::Engine;
use std::time::Duration;

const HEX_MARKER: &str = "##ZLOGV1##";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DecodeReport {
    /// Lines that carried raw dictionary data.
    pub raw_lines: usize,
    /// Bytes fed to the parser from those lines.
    pub raw_bytes: usize,
    /// Dictionary records the parser decoded. Console text it passes through does not count.
    pub records: usize,
    /// Output lines from decoded records and other parser output.
    pub decoded_lines: usize,
    /// Parser notices such as skipped bytes or lost synchronization.
    pub notices: usize,
    /// Raw lines whose Base64 could not be read; kept as they were.
    pub invalid_lines: usize,
}

/// Decode with the dictionary at `database`. Fails when the data does not match it.
pub fn decode_log_with_database(
    input: &str,
    parser_type: &str,
    database: &str,
) -> Result<(String, DecodeReport), String> {
    let mut parser = try_create_dictionary_parser(&ParserConfig {
        parser_type: parser_type.into(),
        database: Some(database.to_string().into()),
    })?;
    decode_log(input, parser.as_mut(), parser_type == "zephyr-dict-hex")
}

/// `flush`: lines released at the end of the input. They are not evidence that the
/// dictionary fits: a parser that never synchronized releases the leftover bytes
/// as junk text.
fn emit(
    lines: Vec<String>,
    prefix: &str,
    out: &mut String,
    report: &mut DecodeReport,
    flush: bool,
) {
    for line in lines {
        if flush || line.starts_with("[zephyr-dict:") {
            report.notices += 1;
        } else {
            report.decoded_lines += 1;
        }
        out.push_str(prefix);
        out.push_str(&line);
        out.push('\n');
    }
}

pub fn decode_log(
    input: &str,
    parser: &mut dyn StreamParser,
    hex: bool,
) -> Result<(String, DecodeReport), String> {
    let mut out = String::with_capacity(input.len());
    let mut report = DecodeReport::default();
    let mut prefix = String::new();

    for original in input.split_inclusive('\n') {
        let line = original.trim_end_matches(['\n', '\r']);
        let found = if hex {
            line.find(HEX_MARKER)
                .map(|at| (at, line.as_bytes()[at..].to_vec()))
        } else {
            line.find(RAW_LINE_PREFIX).map(|at| {
                let payload = line[at + RAW_LINE_PREFIX.len()..].trim();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(payload)
                    .ok();
                (at, bytes.unwrap_or_default())
            })
        };
        let Some((at, mut bytes)) = found else {
            out.push_str(original);
            continue;
        };
        if bytes.is_empty() {
            report.invalid_lines += 1;
            out.push_str(original);
            continue;
        }
        report.raw_lines += 1;
        report.raw_bytes += bytes.len();
        prefix = line[..at].to_string();
        if hex {
            bytes.push(b'\n');
        }
        emit(parser.feed(&bytes), &prefix, &mut out, &mut report, false);
    }
    emit(
        parser.on_idle(Duration::from_secs(6)),
        &prefix,
        &mut out,
        &mut report,
        true,
    );

    report.records = parser.decoded_records();
    if report.raw_bytes > 0 && report.records == 0 {
        return Err(format!(
            "none of the {} raw dictionary lines could be decoded with this dictionary; \
             it does not match the firmware that produced the log (or the target is not \
             little-endian/32-bit)",
            report.raw_lines
        ));
    }
    Ok((out, report))
}
