//! Zephyr dictionary-logging parser (UART HEX wire format).
//!
//! Node firmware shares one UART between the shell and dictionary logging.
//! Dictionary packets are framed as `##ZLOGV1##` followed by ASCII hex; all
//! other bytes are passed through as console text. Hex payloads are unhexified
//! into Zephyr's self-length-prefixed binary message stream and decoded against
//! `log_dictionary.json` from the matching west build.

use std::collections::HashMap;

use chrono::{TimeZone, Utc};
use serde::Deserialize;

use super::traits::StreamParser;

/// Default earliest-valid epoch for UTC log timestamps (2000-01-01 UTC).
const DEFAULT_EARLIEST_VALID_EPOCH: u64 = 946_684_800;

const MSG_TYPE_NORMAL: u8 = 0;
const MSG_TYPE_DROPPED: u8 = 1;
const LOG_HEX_SEP: &str = "##ZLOGV1##";
const LOG_HEX_SEP_BYTES: &[u8] = LOG_HEX_SEP.as_bytes();
/// Minimum trailing lowercase-hex run (chars) to treat as a dictionary burst without
/// an explicit `##ZLOGV1##` prefix (Node sometimes omits the marker between bursts).
const MIN_HEURISTIC_HEX_LEN: usize = 20;
/// Minimum hex-only text buffer length to enter Hex mode right after a shell prompt.
const MIN_PROMPT_HEX_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MuxMode {
    Text,
    Hex,
}

/// Dictionary-logging parser for mixed shell + `##ZLOGV1##` HEX UART captures.
pub struct ZephyrDictParser {
    state: LoadState,
    mux_mode: MuxMode,
    text_buf: Vec<u8>,
    hex_collect: String,
    hex_unhexified: usize,
    hex_parse_stopped: bool,
    dict_buf: Vec<u8>,
    reported_load_error: bool,
}

enum LoadState {
    Ready(Database),
    Failed(String),
}

impl ZephyrDictParser {
    pub fn new(database_path: &str) -> Self {
        let state = match Database::load(database_path) {
            Ok(db) => LoadState::Ready(db),
            Err(e) => {
                tracing::error!("zephyr-dict: failed to load database {database_path:?}: {e}");
                LoadState::Failed(e)
            }
        };
        Self {
            state,
            mux_mode: MuxMode::Text,
            text_buf: Vec::new(),
            hex_collect: String::new(),
            hex_unhexified: 0,
            hex_parse_stopped: false,
            dict_buf: Vec::new(),
            reported_load_error: false,
        }
    }

    fn emit_text_line(&mut self) -> Option<String> {
        let line = String::from_utf8_lossy(&self.text_buf)
            .trim_end_matches('\r')
            .to_string();
        self.text_buf.clear();
        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }

    fn try_flush_prompt_before_hex(&mut self, lines: &mut Vec<String>) {
        if self.text_buf.ends_with(b"outside> ") || self.text_buf.ends_with(b"inside> ") {
            if let Some(line) = self.emit_text_line() {
                lines.push(line);
            }
        }
    }

    fn try_split_on_separator(&mut self, lines: &mut Vec<String>) {
        let Some(pos) = find_subslice(&self.text_buf, LOG_HEX_SEP_BYTES) else {
            return;
        };
        let mut tail = self.text_buf.split_off(pos);
        if let Some(line) = self.emit_text_line() {
            lines.push(line);
        }
        tail.drain(..LOG_HEX_SEP_BYTES.len());
        self.mux_mode = MuxMode::Hex;
        self.reset_hex_collect();
        for b in tail {
            self.handle_hex_byte(b, lines);
        }
    }

    fn try_promote_hex_text_buf(&mut self, lines: &mut Vec<String>) {
        if self.text_buf.is_empty() {
            return;
        }
        if self.text_buf.iter().all(|&b| is_lower_hex(b)) && self.text_buf.len() >= MIN_PROMPT_HEX_LEN
        {
            self.hex_collect = String::from_utf8_lossy(&self.text_buf).into_owned();
            self.hex_unhexified = 0;
            self.text_buf.clear();
            self.mux_mode = MuxMode::Hex;
            self.try_incremental_hex_decode(lines);
            return;
        }

        let n = self.text_buf.len();
        let mut start = n;
        while start > 0 && is_lower_hex(self.text_buf[start - 1]) {
            start -= 1;
        }
        let run_len = n - start;
        if run_len < MIN_HEURISTIC_HEX_LEN {
            return;
        }
        if start > 0 {
            let prev = self.text_buf[start - 1];
            if prev.is_ascii_alphanumeric() && prev != b' ' {
                return;
            }
        }
        let hex_part = self.text_buf.drain(start..).collect::<Vec<_>>();
        if let Some(line) = self.emit_text_line() {
            lines.push(line);
        }
        self.hex_collect = String::from_utf8_lossy(&hex_part).into_owned();
        self.hex_unhexified = 0;
        self.mux_mode = MuxMode::Hex;
        self.try_incremental_hex_decode(lines);
    }

    fn reset_hex_collect(&mut self) {
        self.hex_collect.clear();
        self.hex_unhexified = 0;
        self.hex_parse_stopped = false;
    }

    fn try_incremental_hex_decode(&mut self, lines: &mut Vec<String>) {
        if self.hex_parse_stopped {
            return;
        }
        loop {
            let pending = &self.hex_collect[self.hex_unhexified..];
            if pending.len() < 2 {
                break;
            }
            let even = pending.len() - (pending.len() % 2);
            let Some(bytes) = unhexify_chunk(&pending[..even], 2) else {
                break;
            };
            self.hex_unhexified += even;
            self.dict_buf.extend(bytes);
            lines.extend(self.decode_pending_dict_messages());
        }
    }

    fn handle_hex_byte(&mut self, b: u8, lines: &mut Vec<String>) {
        if is_lower_hex(b) {
            self.hex_collect.push(b as char);
            self.try_incremental_hex_decode(lines);
        } else if matches!(b, b'\r' | b'\n') {
            if !self.hex_collect.is_empty() {
                self.flush_hex_block(lines);
            }
            self.mux_mode = MuxMode::Text;
        } else if b == b'\t' || b == b' ' {
            // skip framing whitespace between separator and hex body
        } else if b == b'#' {
            self.flush_hex_block(lines);
            self.mux_mode = MuxMode::Text;
            self.text_buf.push(b);
            self.try_split_on_separator(lines);
        } else {
            self.flush_hex_block(lines);
            self.mux_mode = MuxMode::Text;
            self.handle_text_byte(b, lines);
        }
    }

    fn handle_text_byte(&mut self, b: u8, lines: &mut Vec<String>) {
        if b == b'\n' || b == b'\r' {
            if let Some(line) = self.emit_text_line() {
                lines.push(line);
            }
            return;
        }
        if is_lower_hex(b) {
            self.try_flush_prompt_before_hex(lines);
        }
        self.text_buf.push(b);
        self.try_split_on_separator(lines);
        if self.mux_mode == MuxMode::Text {
            self.try_promote_hex_text_buf(lines);
        }
    }

    fn flush_hex_block(&mut self, lines: &mut Vec<String>) {
        self.try_incremental_hex_decode(lines);
        self.reset_hex_collect();
    }

    fn decode_pending_dict_messages(&mut self) -> Vec<String> {
        let LoadState::Ready(db) = &self.state else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        loop {
            match take_one_message(&self.dict_buf, db) {
                TakeResult::NeedMore => break,
                TakeResult::Consumed { len, mut output } => {
                    self.dict_buf.drain(0..len);
                    lines.append(&mut output);
                }
                TakeResult::Stop => {
                    self.dict_buf.clear();
                    self.hex_parse_stopped = true;
                    break;
                }
            }
        }
        lines
    }

    fn process_byte(&mut self, b: u8, lines: &mut Vec<String>) {
        match self.mux_mode {
            MuxMode::Text => self.handle_text_byte(b, lines),
            MuxMode::Hex => self.handle_hex_byte(b, lines),
        }
    }
}

impl StreamParser for ZephyrDictParser {
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        if let LoadState::Failed(err) = &self.state {
            if self.reported_load_error {
                return Vec::new();
            }
            self.reported_load_error = true;
            return vec![format!(
                "[zephyr-dict: database not loaded ({err}); check parser.database — dropping incoming bytes]"
            )];
        }

        let mut lines = Vec::new();
        for &b in data {
            self.process_byte(b, &mut lines);
        }
        lines
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn is_lower_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f')
}

fn unhexify_chunk(hexstr: &str, min_hex_len: usize) -> Option<Vec<u8>> {
    if hexstr.len() < min_hex_len.max(2) {
        return None;
    }
    let mut end = hexstr.len();
    if end % 2 == 1 {
        end -= 1;
    }
    while end > 0 {
        let mut out = Vec::with_capacity(end / 2);
        let mut failed = false;
        for pair in hexstr[..end].as_bytes().chunks(2) {
            if pair.len() != 2 {
                failed = true;
                break;
            }
            let Some(hi) = hex_nibble(&pair[0]) else {
                failed = true;
                break;
            };
            let Some(lo) = hex_nibble(&pair[1]) else {
                failed = true;
                break;
            };
            out.push((hi << 4) | lo);
        }
        if !failed {
            return Some(out);
        }
        end -= 2;
    }
    None
}

fn hex_nibble(byte: &u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Normalize format strings from the dictionary database for Rust `sprintf`.
///
/// Mirrors Zephyr's `log_parser.py::formalize_fmt_string` plus `inttypes.h` /
/// `size_t` macros that may appear literally in `log_strings` ELF entries.
fn formalize_fmt_string(fmt: &str, is_64bit: bool) -> String {
    let mut s = fmt.to_string();

    let (zu, zd, zi) = if is_64bit {
        ("%lu", "%ld", "%ld")
    } else {
        ("%u", "%d", "%d")
    };
    for (src, dst) in [
        ("%PRIu32", "%u"),
        ("%PRId32", "%d"),
        ("%PRIx32", "%x"),
        ("%PRIu64", "%llu"),
        ("%PRId64", "%lld"),
        ("%PRIx64", "%llx"),
        ("%zu", zu),
        ("%zd", zd),
        ("%zi", zi),
    ] {
        s = s.replace(src, dst);
    }

    for spec in ['d', 'i', 'o', 'u', 'x', 'X'] {
        s = s.replace(&format!("%ll{spec}"), &format!("%l{spec}"));
        if matches!(spec, 'x' | 'X') {
            s = s.replace(&format!("%#ll{spec}"), &format!("%#l{spec}"));
        }
        s = s.replace(&format!("%hh{spec}"), &format!("%h{spec}"));
    }
    s.replace("%p", "0x%x")
}

enum TakeResult {
    NeedMore,
    Consumed { len: usize, output: Vec<String> },
    /// Malformed message or decode failure — stop parsing, matching upstream
    /// `log_parser_v3.py` (returns `False` / `None` without advancing).
    Stop,
}

fn take_one_message(buf: &[u8], db: &Database) -> TakeResult {
    let Some(&msg_type) = buf.first() else {
        return TakeResult::NeedMore;
    };

    match msg_type {
        MSG_TYPE_DROPPED => {
            let total = 1 + 2;
            if buf.len() < total {
                return TakeResult::NeedMore;
            }
            let Some(count) = read_uint_sized(&buf[1..3], db.little_endian, 2) else {
                return TakeResult::Stop;
            };
            TakeResult::Consumed {
                len: total,
                output: vec![format!("--- {count} messages dropped ---")],
            }
        }
        MSG_TYPE_NORMAL => take_normal_message(buf, db),
        _ => TakeResult::Stop,
    }
}

fn take_normal_message(buf: &[u8], db: &Database) -> TakeResult {
    let ptr_size = db.ptr_size();
    let ts_size = if db.timestamp_64bit { 8 } else { 4 };
    // 1 (type) + domain_lvl(1) + pkg_len(2) + data_len(2) + source(ptr) + timestamp(ts)
    let fixed_prefix = 1 + 1 + 2 + 2 + ptr_size + ts_size;

    if buf.len() < fixed_prefix {
        return TakeResult::NeedMore;
    }

    let domain_lvl = buf[1];
    let malformed = || TakeResult::Stop;
    let Some(pkg_len) = read_uint_sized(&buf[2..4], db.little_endian, 2) else {
        return malformed();
    };
    let Some(data_len) = read_uint_sized(&buf[4..6], db.little_endian, 2) else {
        return malformed();
    };
    let source_off = 6;
    let Some(source_id) = read_uint_sized(&buf[source_off..source_off + ptr_size], db.little_endian, ptr_size)
    else {
        return malformed();
    };
    let ts_off = source_off + ptr_size;
    let Some(timestamp) = read_uint_sized(&buf[ts_off..ts_off + ts_size], db.little_endian, ts_size) else {
        return malformed();
    };

    let total_len = fixed_prefix + pkg_len as usize + data_len as usize;
    if buf.len() < total_len {
        return TakeResult::NeedMore;
    }

    // Matches log_parser_v3.py: bit layout of domain/level flips with endianness.
    let (domain_id, level) = if db.little_endian {
        ((domain_lvl & 0x0F) as u32, ((domain_lvl >> 4) & 0x0F) as u32)
    } else {
        (((domain_lvl >> 4) & 0x0F) as u32, (domain_lvl & 0x0F) as u32)
    };
    if level > 4 {
        return TakeResult::Stop;
    }

    let package = &buf[fixed_prefix..fixed_prefix + pkg_len as usize];
    let extra_data = &buf[fixed_prefix + pkg_len as usize..total_len];

    let body = match decode_package(package, db) {
        Ok(msg) => msg,
        Err(_) => return TakeResult::Stop,
    };

    let mut output = Vec::with_capacity(2);
    if level == 0 {
        push_display_lines(&mut output, None, &body);
    } else {
        let source_name = db.source_name(domain_id, source_id);
        let ts_prefix = db.format_log_timestamp(timestamp);
        let prefix = format!("<{}> {source_name}: ", level_name(level));
        push_display_lines(&mut output, Some(&format!("{ts_prefix}{prefix}")), &body);
    }
    if !extra_data.is_empty() {
        output.extend(hexdump_lines(extra_data));
    }

    TakeResult::Consumed {
        len: total_len,
        output,
    }
}

fn level_name(level: u32) -> &'static str {
    match level {
        0 => "none",
        1 => "err",
        2 => "wrn",
        3 => "inf",
        4 => "dbg",
        _ => "unk",
    }
}

fn push_display_lines(output: &mut Vec<String>, prefix: Option<&str>, body: &str) {
    // Virtual-scroll UI uses a fixed row height per entry; embedded '\n' in one
    // message would paint multiple text lines into a single row and overlap
    // the next entry. Split on newlines so each row is one visual line.
    let prefix = prefix.unwrap_or("");
    for part in body.split('\n') {
        let part = part.trim_end();
        if part.is_empty() {
            continue;
        }
        output.push(format!("{prefix}{part}"));
    }
}

fn hexdump_lines(data: &[u8]) -> Vec<String> {
    const PER_LINE: usize = 16;
    data.chunks(PER_LINE)
        .map(|chunk| {
            let hex: String = chunk.iter().map(|b| format!("{b:02x} ")).collect();
            let ascii: String = chunk
                .iter()
                .map(|&b| if (32..=126).contains(&b) { b as char } else { '.' })
                .collect();
            format!("    {hex:<48}|{ascii}")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Package decoding: cbprintf_package layout -> format string + argument list
// ---------------------------------------------------------------------------

fn decode_package(package: &[u8], db: &Database) -> Result<String, String> {
    if package.len() < 4 {
        return Err("package shorter than its 4-byte sub-header".to_string());
    }
    let ptr_size = db.ptr_size();

    let offset_end_of_args_units = package[0] as i64;
    let num_packed_strings = package[1] as usize;
    let num_ro_str_indexes = package[2] as i64;
    let num_rw_str_indexes = package[3] as i64;

    let offset_end_of_args =
        offset_end_of_args_units * 4 + num_ro_str_indexes + num_rw_str_indexes;
    if offset_end_of_args < 0 || offset_end_of_args as usize > package.len() {
        return Err("argument-list end offset out of range".to_string());
    }
    let offset_end_of_args = offset_end_of_args as usize;

    let string_tbl = extract_string_table(&package[offset_end_of_args..]);
    if string_tbl.len() != num_packed_strings {
        return Err("packed string table size mismatch".to_string());
    }

    let hdr_len = 4 + ptr_size; // 4-byte sub-header + format-string pointer
    if package.len() < hdr_len || offset_end_of_args < hdr_len {
        return Err("package too short for format-string pointer".to_string());
    }
    let fmt_ptr_off = 4;
    let fmt_str_ptr = read_uint_sized(&package[fmt_ptr_off..fmt_ptr_off + ptr_size], db.little_endian, ptr_size)
        .ok_or("failed to read format-string pointer")?;
    // Negative offset: the format string sits one pointer-width *before*
    // where the va_list argument offsets are measured from.
    let fmt_str = get_string(db, fmt_str_ptr, -(ptr_size as i64), &string_tbl);
    if fmt_str.is_empty() {
        return Err(format!("could not resolve format string @0x{fmt_str_ptr:x}"));
    }

    let arg_list = &package[hdr_len..offset_end_of_args];
    let args = extract_args(&fmt_str, arg_list, &string_tbl, db)
        .ok_or_else(|| format!("failed to extract arguments for format {fmt_str:?}"))?;

    render(&fmt_str, &args)
}

fn extract_string_table(bytes: &[u8]) -> HashMap<i64, String> {
    let mut tbl = HashMap::new();
    let mut idx: Option<i64> = None;
    let mut cur = String::new();
    for &b in bytes {
        match idx {
            None => idx = Some(b as i64),
            Some(i) => {
                if b == 0 {
                    tbl.insert(i, std::mem::take(&mut cur));
                    idx = None;
                } else {
                    cur.push(b as char);
                }
            }
        }
    }
    tbl
}

/// Resolve a string pointer: try the database's static string tables first,
/// then fall back to the packed message's own inline string table (used for
/// strings the firmware couldn't statically resolve at compile time).
fn get_string(db: &Database, ptr: u64, arg_offset: i64, string_tbl: &HashMap<i64, String>) -> String {
    if let Some(s) = db.find_string(ptr) {
        return s;
    }
    let ptr_size = db.ptr_size() as i64;
    let str_idx = (arg_offset + ptr_size * 2) / 4; // 4 == sizeof(int), always
    match string_tbl.get(&str_idx) {
        Some(s) => s.clone(),
        None => format!("<string@0x{ptr:x}>"),
    }
}

// ---------------------------------------------------------------------------
// va_list argument extraction (ports cbvprintf_package's argument walk)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgType {
    Int,
    UInt,
    Long,
    ULong,
    LongLong,
    ULongLong,
    Ptr,
    Double,
    LongDouble,
}

enum ArgVal {
    I(i64),
    U(u64),
    F(f64),
    S(String),
}

struct ExtractedArg {
    conv: char,
    value: ArgVal,
}

/// sizeof() for skip/alignment purposes (LongDouble is deliberately
/// oversized to 16 — matches the Python tool, which can't decode real long
/// double but still needs to skip the right number of bytes).
fn type_size(t: ArgType, is64: bool) -> usize {
    match t {
        ArgType::Int | ArgType::UInt => 4,
        ArgType::Long | ArgType::ULong => {
            if is64 {
                8
            } else {
                4
            }
        }
        ArgType::LongLong | ArgType::ULongLong => 8,
        ArgType::Ptr => {
            if is64 {
                8
            } else {
                4
            }
        }
        ArgType::Double => 8,
        ArgType::LongDouble => 16,
    }
}

/// Bytes actually read to decode the value (differs from `type_size` only
/// for LongDouble, which is read as an 8-byte f64 approximation).
fn value_read_size(t: ArgType, is64: bool) -> usize {
    if t == ArgType::LongDouble {
        8
    } else {
        type_size(t, is64)
    }
}

fn type_align(t: ArgType, is64: bool) -> usize {
    let base = if is64 { 8 } else { 4 };
    base.max(type_size(t, is64))
}

/// Port of `DataTypes.get_stack_min_align`: (min alignment, needs further
/// per-type refinement) for a given architecture string.
fn stack_min_align(arch: &str, is64: bool) -> (usize, bool) {
    match arch {
        "arc" => {
            if is64 {
                (8, true)
            } else {
                (1, false)
            }
        }
        "arm64" => (8, true),
        "sparc" => (1, false),
        "x86" => {
            if is64 {
                (8, true)
            } else {
                (1, false)
            }
        }
        "riscv32e" => (1, false),
        "riscv" => (if is64 { 8 } else { 1 }, true),
        "nios2" => (1, false),
        _ => (1, true),
    }
}

fn data_type_align_override(t: ArgType, is64: bool) -> usize {
    match t {
        ArgType::LongLong | ArgType::ULongLong => 8,
        ArgType::Long | ArgType::ULong => {
            if is64 {
                8
            } else {
                4
            }
        }
        _ => 4,
    }
}

fn stack_align(arch: &str, is64: bool, t: ArgType) -> usize {
    let (base, need_more) = stack_min_align(arch, is64);
    if need_more {
        data_type_align_override(t, is64)
    } else {
        base
    }
}

/// Port of `process_one_fmt_str`: walk a printf-style format string and pull
/// typed arguments out of the raw va_list byte blob.
// The default-then-overwrite assignments below mirror the Python original's
// structure (easier to audit against it); harmless dead stores.
#[allow(unused_assignments)]
fn extract_args(
    fmt: &str,
    arg_list: &[u8],
    string_tbl: &HashMap<i64, String>,
    db: &Database,
) -> Option<Vec<ExtractedArg>> {
    let is64 = db.is_64bit;
    let le = db.little_endian;
    let arch = db.arch.as_str();

    let chars: Vec<char> = fmt.chars().collect();
    let mut arg_offset: i64 = 0;
    let mut arg_data_type = ArgType::Int;
    let mut is_parsing = false;
    let mut args = Vec::new();

    let mut i = 0usize;
    while i < chars.len() {
        let fmt_ch = chars[i];
        let mut do_extract = false;

        if !is_parsing {
            if fmt_ch == '%' {
                is_parsing = true;
                arg_data_type = ArgType::Int;
            }
            i += 1;
            continue;
        } else if fmt_ch == '%' {
            is_parsing = false;
            i += 1;
            continue;
        } else if fmt_ch == '*' {
            // Match log_parser_v3.py: '*' is a modifier, not a separate argument.
            i += 1;
            continue;
        } else if fmt_ch.is_ascii_digit()
            || fmt_ch == 'l'
            || fmt_ch == 'L'
            || matches!(fmt_ch, ' ' | '#' | '-' | '+' | '.' | 'h')
        {
            i += 1;
            continue;
        } else if matches!(fmt_ch, 'j' | 'z' | 't') {
            arg_data_type = ArgType::Long;
            i += 1;
            continue;
        } else if matches!(fmt_ch, 'c' | 'd' | 'i' | 'o' | 'u' | 'x' | 'X') {
            let unsigned = matches!(fmt_ch, 'c' | 'o' | 'u' | 'x' | 'X');
            let prev1 = (i >= 1).then(|| chars[i - 1]);
            let prev2 = (i >= 2).then(|| chars[i - 2]);
            arg_data_type = if prev1 == Some('l') {
                if prev2 == Some('l') {
                    if unsigned {
                        ArgType::ULongLong
                    } else {
                        ArgType::LongLong
                    }
                } else if unsigned {
                    ArgType::ULong
                } else {
                    ArgType::Long
                }
            } else if unsigned {
                ArgType::UInt
            } else {
                ArgType::Int
            };
            is_parsing = false;
            do_extract = true;
        } else if matches!(fmt_ch, 's' | 'p' | 'n') {
            arg_data_type = ArgType::Ptr;
            is_parsing = false;
            do_extract = true;
        } else if matches!(fmt_ch.to_ascii_lowercase(), 'a' | 'e' | 'f' | 'g') {
            let prev1 = (i >= 1).then(|| chars[i - 1]);
            arg_data_type = if prev1 == Some('L') {
                ArgType::LongDouble
            } else {
                ArgType::Double
            };
            is_parsing = false;
            do_extract = true;
        } else {
            is_parsing = false;
            i += 1;
            continue;
        }

        if do_extract {
            let align = type_align(arg_data_type, is64) as i64;
            let skip_size = type_size(arg_data_type, is64) as i64;
            let read_size = value_read_size(arg_data_type, is64);
            let stk_align = stack_align(arch, is64, arg_data_type) as i64;

            if stk_align > 1 {
                arg_offset = ((arg_offset + (align - 1)) / align) * align;
            }

            let off = usize::try_from(arg_offset).ok()?;
            let raw = arg_list.get(off..off + read_size)?;

            let value = match arg_data_type {
                ArgType::Int => ArgVal::I(read_int_sized(raw, le, read_size)?),
                ArgType::UInt => ArgVal::U(read_uint_sized(raw, le, read_size)?),
                ArgType::Long | ArgType::LongLong => ArgVal::I(read_int_sized(raw, le, read_size)?),
                ArgType::ULong | ArgType::ULongLong => ArgVal::U(read_uint_sized(raw, le, read_size)?),
                ArgType::Ptr => ArgVal::U(read_uint_sized(raw, le, read_size)?),
                ArgType::Double | ArgType::LongDouble => ArgVal::F(read_f64(raw, le)?),
            };

            let value = if fmt_ch == 's' {
                let ptr = match value {
                    ArgVal::U(v) => v,
                    _ => return None,
                };
                ArgVal::S(get_string(db, ptr, arg_offset, string_tbl))
            } else {
                value
            };

            args.push(ExtractedArg {
                conv: fmt_ch,
                value,
            });

            arg_offset += skip_size;
            if stk_align > 1 {
                arg_offset = ((arg_offset + align - 1) / align) * align;
            }
        }

        i += 1;
    }

    Some(args)
}

fn render(fmt_str: &str, args: &[ExtractedArg]) -> Result<String, String> {
    let boxed: Vec<Box<dyn sprintf::Printf>> = args.iter().map(arg_to_printf_box).collect();
    let refs: Vec<&dyn sprintf::Printf> = boxed.iter().map(|b| b.as_ref()).collect();
    match sprintf::vsprintf(fmt_str, &refs) {
        Ok(s) => Ok(s),
        // Dynamic width/precision ('*') and other unsupported specifiers: show
        // the format string rather than a second overlapping diagnostic line.
        Err(_) if fmt_str.contains('*') => Ok(fmt_str.to_string()),
        Err(e) => Err(format!("{e} (fmt={fmt_str:?})")),
    }
}

fn arg_to_printf_box(arg: &ExtractedArg) -> Box<dyn sprintf::Printf> {
    match (&arg.value, arg.conv) {
        (ArgVal::S(s), _) => Box::new(s.clone()),
        // %c needs a concrete type sprintf's Printf impl treats as a
        // character (u32 handles ConversionType::Char); everything else
        // (%d/%u/%x/%o/%p/...) works fine as plain i64/u64.
        (ArgVal::I(v), 'c') => Box::new(*v as u32),
        (ArgVal::U(v), 'c') => Box::new(*v as u32),
        (ArgVal::I(v), _) => Box::new(*v),
        (ArgVal::U(v), _) => Box::new(*v),
        (ArgVal::F(v), _) => Box::new(*v),
    }
}

fn read_int_sized(b: &[u8], le: bool, width: usize) -> Option<i64> {
    match width {
        4 => {
            let a: [u8; 4] = b.get(0..4)?.try_into().ok()?;
            Some(i64::from(if le {
                i32::from_le_bytes(a)
            } else {
                i32::from_be_bytes(a)
            }))
        }
        8 => {
            let a: [u8; 8] = b.get(0..8)?.try_into().ok()?;
            Some(if le {
                i64::from_le_bytes(a)
            } else {
                i64::from_be_bytes(a)
            })
        }
        _ => None,
    }
}

fn read_uint_sized(b: &[u8], le: bool, width: usize) -> Option<u64> {
    match width {
        2 => {
            let a: [u8; 2] = b.get(0..2)?.try_into().ok()?;
            Some(u64::from(if le {
                u16::from_le_bytes(a)
            } else {
                u16::from_be_bytes(a)
            }))
        }
        4 => {
            let a: [u8; 4] = b.get(0..4)?.try_into().ok()?;
            Some(u64::from(if le {
                u32::from_le_bytes(a)
            } else {
                u32::from_be_bytes(a)
            }))
        }
        8 => {
            let a: [u8; 8] = b.get(0..8)?.try_into().ok()?;
            Some(if le {
                u64::from_le_bytes(a)
            } else {
                u64::from_be_bytes(a)
            })
        }
        _ => None,
    }
}

fn read_f64(b: &[u8], le: bool) -> Option<f64> {
    let a: [u8; 8] = b.get(0..8)?.try_into().ok()?;
    Some(if le {
        f64::from_le_bytes(a)
    } else {
        f64::from_be_bytes(a)
    })
}

// ---------------------------------------------------------------------------
// database.json
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawDatabase {
    version: u32,
    #[serde(default)]
    target: RawTarget,
    #[serde(default)]
    arch: String,
    #[serde(default)]
    kconfigs: HashMap<String, serde_json::Value>,
    #[serde(default)]
    log_subsys: RawLogSubsys,
    #[serde(default)]
    string_mappings: HashMap<String, String>,
    #[serde(default)]
    sections: HashMap<String, RawSection>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTarget {
    bits: Option<u32>,
    little_endianness: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLogSubsys {
    #[serde(default)]
    log_instances: HashMap<String, RawLogInstance>,
}

#[derive(Debug, Deserialize)]
struct RawLogInstance {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawSection {
    start: u64,
    size: u64,
    data_b64: String,
}

struct Section {
    start: u64,
    size: u64,
    data: Vec<u8>,
}

struct Database {
    is_64bit: bool,
    little_endian: bool,
    arch: String,
    timestamp_64bit: bool,
    /// Epoch seconds at/above which log timestamps are shown as UTC.
    earliest_valid_epoch: u64,
    /// source_id (as it appears in the JSON, i.e. its decimal string) -> name
    log_instances: HashMap<String, String>,
    string_mappings: HashMap<u64, String>,
    sections: Vec<Section>,
}

impl Database {
    fn ptr_size(&self) -> usize {
        if self.is_64bit {
            8
        } else {
            4
        }
    }

    fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path:?}: {e}"))?;
        let raw: RawDatabase =
            serde_json::from_str(&text).map_err(|e| format!("parse {path:?}: {e}"))?;

        if raw.version != 3 {
            return Err(format!(
                "unsupported database version {} (only version 3 is supported)",
                raw.version
            ));
        }

        let is_64bit = raw.target.bits.unwrap_or(32) == 64;
        let mut string_mappings = HashMap::with_capacity(raw.string_mappings.len());
        for (addr, s) in raw.string_mappings {
            if let Ok(addr) = addr.parse::<u64>() {
                let value = formalize_fmt_string(&s, is_64bit);
                string_mappings.insert(addr, value);
            }
        }

        let mut sections = Vec::with_capacity(raw.sections.len());
        for (_, sect) in raw.sections {
            use base64::Engine;
            let data = base64::engine::general_purpose::STANDARD
                .decode(sect.data_b64)
                .map_err(|e| format!("decode section data: {e}"))?;
            sections.push(Section {
                start: sect.start,
                size: sect.size,
                data,
            });
        }

        let log_instances = raw
            .log_subsys
            .log_instances
            .into_iter()
            .map(|(id, inst)| (id, inst.name))
            .collect();

        let earliest_valid_epoch = parse_kconfig_u64(&raw.kconfigs, "CONFIG_APP_TIMEMGR_EARLIEST_VALID_DATE")
            .unwrap_or(DEFAULT_EARLIEST_VALID_EPOCH);

        Ok(Self {
            is_64bit: raw.target.bits.unwrap_or(32) == 64,
            little_endian: raw.target.little_endianness.unwrap_or(true),
            arch: raw.arch,
            timestamp_64bit: raw.kconfigs.contains_key("CONFIG_LOG_TIMESTAMP_64BIT"),
            earliest_valid_epoch,
            log_instances,
            string_mappings,
            sections,
        })
    }

    fn format_log_timestamp(&self, timestamp: u64) -> String {
        format_log_timestamp(timestamp, self.timestamp_64bit, self.earliest_valid_epoch)
    }

    fn find_string(&self, ptr: u64) -> Option<String> {
        if let Some(s) = self.string_mappings.get(&ptr) {
            return Some(s.clone());
        }
        // Combined/deduplicated strings: ptr may point partway into an
        // already-stored string sharing a suffix with it.
        for (addr, s) in &self.string_mappings {
            if *addr <= ptr && ptr < addr + s.len() as u64 {
                let offset = (ptr - addr) as usize;
                return Some(s[offset..].to_string());
            }
        }
        for section in &self.sections {
            if ptr < section.start {
                continue;
            }
            let offset = ptr - section.start;
            if offset >= section.size || offset as usize >= section.data.len() {
                continue;
            }
            let offset = offset as usize;
            let end = section.data[offset..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| offset + p)
                .unwrap_or(section.data.len());
            return Some(String::from_utf8_lossy(&section.data[offset..end]).into_owned());
        }
        None
    }

    fn source_name(&self, domain_id: u32, source_id: u64) -> String {
        self.log_instances
            .get(&source_id.to_string())
            .cloned()
            .unwrap_or_else(|| format!("unknown<{domain_id}:{source_id}>"))
    }
}

fn parse_kconfig_u64(kconfigs: &HashMap<String, serde_json::Value>, key: &str) -> Option<u64> {
    let value = kconfigs.get(key)?;
    if let Some(n) = value.as_u64() {
        return Some(n);
    }
    if let Some(n) = value.as_i64() {
        return u64::try_from(n).ok();
    }
    value.as_str()?.parse().ok()
}

/// Format a Zephyr dictionary log timestamp for human-readable decode output.
fn format_log_timestamp(timestamp: u64, timestamp_64bit: bool, earliest_valid_epoch: u64) -> String {
    if timestamp < earliest_valid_epoch {
        if timestamp_64bit {
            return format!("[{timestamp:016}] ");
        }
        return format!("[{timestamp:08}] ");
    }

    let Ok(secs) = i64::try_from(timestamp) else {
        return format!("[{timestamp}] ");
    };
    match Utc.timestamp_opt(secs, 0).single() {
        Some(dt) => format!("[{}]: ", dt.format("%Y-%m-%dT%H:%M:%SZ")),
        None => format!("[{timestamp}] "),
    }
}

#[cfg(test)]
mod tests {
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

        let mut package = Vec::new();
        package.push(end_of_args_units);
        package.push(0); // num_packed_strings
        package.push(0); // num_ro_str_indexes
        package.push(0); // num_rw_str_indexes
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

    fn feed_hex(parser: &mut ZephyrDictParser, msg: &[u8]) -> Vec<String> {
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let line = format!("{LOG_HEX_SEP}{hex}\n");
        parser.feed(line.as_bytes())
    }

    #[test]
    fn decodes_simple_string_only_message() {
        let dir = temp_dir("simple");
        // fmt string lives at address 0x1000
        let db_path = write_test_db(&dir, &[(0x1000, "hello world\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 42, 0x1000, &[], &[]);
        let lines = feed_hex(&mut parser, &msg);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("<inf> app: hello world"), "{}", lines[0]);
        assert!(lines[0].contains("42"), "{}", lines[0]);
    }

    #[test]
    fn decodes_message_with_integer_args() {
        let dir = temp_dir("intargs");
        let db_path = write_test_db(&dir, &[(0x2000, "count=%d hex=%#x\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let mut args = Vec::new();
        args.extend_from_slice(&(-5i32).to_le_bytes());
        args.extend_from_slice(&(255u32).to_le_bytes());
        let msg = encode_normal_message(1, 1, 7, 0x2000, &args, &[]);

        let lines = feed_hex(&mut parser, &msg);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("count=-5 hex=0xff"), "{}", lines[0]);
        assert!(lines[0].contains("<err>"), "{}", lines[0]);
    }

    #[test]
    fn decodes_message_with_string_arg() {
        let dir = temp_dir("strarg");
        let db_path = write_test_db(&dir, &[(0x3000, "name=%s\n"), (0x4000, "widget")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let mut args = Vec::new();
        args.extend_from_slice(&(0x4000u32).to_le_bytes());
        let msg = encode_normal_message(3, 1, 1, 0x3000, &args, &[]);

        let lines = feed_hex(&mut parser, &msg);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("name=widget"), "{}", lines[0]);
    }

    #[test]
    fn level_none_message_has_no_prefix() {
        let dir = temp_dir("none-level");
        let db_path = write_test_db(&dir, &[(0x1000, "raw printk\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(0, 1, 1, 0x1000, &[], &[]);
        let lines = feed_hex(&mut parser, &msg);
        assert_eq!(lines, vec!["raw printk"]);
    }

    #[test]
    fn dropped_message_reports_count() {
        let dir = temp_dir("dropped");
        let db_path = write_test_db(&dir, &[]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let mut msg = vec![MSG_TYPE_DROPPED];
        msg.extend_from_slice(&7u16.to_le_bytes());
        let lines = feed_hex(&mut parser, &msg);
        assert_eq!(lines, vec!["--- 7 messages dropped ---"]);
    }

    #[test]
    fn reassembles_message_split_across_feed_calls() {
        let dir = temp_dir("split");
        let db_path = write_test_db(&dir, &[(0x1000, "hello\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let line = format!("{LOG_HEX_SEP}{hex}\n");
        let (a, b) = line.split_at(line.len() / 2);

        assert!(parser.feed(a.as_bytes()).is_empty());
        let lines = parser.feed(b.as_bytes());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("hello"));
    }

    #[test]
    fn hexdump_appended_when_data_len_present() {
        let dir = temp_dir("hexdump");
        let db_path = write_test_db(&dir, &[(0x1000, "dump:\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let extra = vec![0xABu8; 20];
        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &extra);
        let lines = feed_hex(&mut parser, &msg);

        // 1 message line + 2 hexdump lines (16 + 4 bytes)
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("ab ab"));
    }

    #[test]
    fn unknown_message_type_stops_parsing() {
        let dir = temp_dir("desync");
        let db_path = write_test_db(&dir, &[(0x1000, "hello\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let mut buf = vec![0xFF]; // unknown type
        buf.extend_from_slice(&encode_normal_message(3, 1, 1, 0x1000, &[], &[]));
        let lines = feed_hex(&mut parser, &buf);
        assert!(lines.is_empty(), "expected stop before any decoded line, got: {lines:?}");
    }

    #[test]
    fn decodes_hex_wire_format_message() {
        let dir = temp_dir("hexwire");
        let db_path = write_test_db(&dir, &[(0x1000, "hello world\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 42, 0x1000, &[], &[]);
        let hex_body: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let hex_line = format!("{LOG_HEX_SEP}\r\n{hex_body}\n");
        let lines = parser.feed(hex_line.as_bytes());

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("<inf> app: hello world"), "{}", lines[0]);
    }

    #[test]
    fn mixed_shell_and_dict() {
        let dir = temp_dir("mixed");
        let db_path = write_test_db(&dir, &[(0x1000, "boot ok\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let input = format!("node outside> help\n{LOG_HEX_SEP}{hex}Available commands:\n");
        let lines = parser.feed(input.as_bytes());

        assert!(lines.iter().any(|l| l.contains("node outside> help")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("boot ok")), "{lines:?}");
        assert!(lines.iter().any(|l| l == "Available commands:"), "{lines:?}");
    }

    #[test]
    fn separator_split_across_feed_calls() {
        let dir = temp_dir("sep-split");
        let db_path = write_test_db(&dir, &[(0x1000, "split ok\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let line = format!("prefix\n{LOG_HEX_SEP}{hex}\n");
        let sep_start = line.find(LOG_HEX_SEP).unwrap();
        let (a, b) = line.split_at(sep_start + LOG_HEX_SEP.len());

        let first = parser.feed(a.as_bytes());
        assert!(first.iter().any(|l| l == "prefix"));
        let second = parser.feed(b.as_bytes());
        assert!(second.iter().any(|l| l.contains("split ok")), "{second:?}");
    }

    #[test]
    fn false_separator_prefix_passes_through_as_text() {
        let dir = temp_dir("false-sep");
        let db_path = write_test_db(&dir, &[(0x1000, "unused\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let lines = parser.feed(b"noise ##ZLO not a frame\n");
        assert_eq!(lines, vec!["noise ##ZLO not a frame"]);
    }

    #[test]
    fn hex_then_prompt_emits_on_carriage_return() {
        let dir = temp_dir("hex-prompt");
        let db_path = write_test_db(&dir, &[(0x1000, "logged\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let input = format!("{LOG_HEX_SEP}{hex}node outside> \r");
        let lines = parser.feed(input.as_bytes());

        assert!(lines.iter().any(|l| l.contains("logged")), "{lines:?}");
        assert!(lines.iter().any(|l| l.trim() == "node outside>"), "{lines:?}");
    }

    #[test]
    fn prompt_then_hex_without_separator_decodes_dict() {
        let dir = temp_dir("prompt-hex-nosep");
        let db_path = write_test_db(&dir, &[(0x1000, "network ok\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let input = format!("node outside> {hex}help");
        let lines = parser.feed(input.as_bytes());

        assert!(
            lines.iter().any(|l| l.trim() == "node outside>"),
            "prompt not emitted separately: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("network ok")),
            "dict not decoded: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.contains("node outside> 00")),
            "hex leaked into text line: {lines:?}"
        );
    }

    #[test]
    fn separator_in_middle_of_text_buffer() {
        let dir = temp_dir("sep-middle");
        let db_path = write_test_db(&dir, &[(0x1000, "mid ok\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let input = format!("noise before {LOG_HEX_SEP}{hex}\n");
        let lines = parser.feed(input.as_bytes());

        assert!(lines.iter().any(|l| l.trim_end() == "noise before"), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("mid ok")), "{lines:?}");
    }

    #[test]
    fn continuous_hex_without_newline_decodes_incrementally() {
        let dir = temp_dir("cont-hex");
        let db_path = write_test_db(&dir, &[(0x1000, "incremental ok\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let lines = parser.feed(hex.as_bytes());

        assert!(
            lines.iter().any(|l| l.contains("incremental ok")),
            "expected decoded line, got {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.chars().all(|c| c.is_ascii_hexdigit())),
            "raw hex leaked: {lines:?}"
        );
    }

    #[test]
    fn node_boot_prefix_hex_decodes_without_separator() {
        let dir = temp_dir("node-prefix");
        let db_path = write_test_db(&dir, &[(0x0e00_2ef8, "node boot ok\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x0e00_2ef8, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let input = format!("{hex}*** Booting Zephyr OS build abc ***\n");
        let lines = parser.feed(input.as_bytes());

        assert!(
            lines.iter().any(|l| l.contains("node boot ok")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("Booting Zephyr")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.starts_with(&hex[..8])),
            "raw hex leaked: {lines:?}"
        );
    }

    #[test]
    fn format_log_timestamp_uptime_and_epoch() {
        assert_eq!(
            format_log_timestamp(12, false, DEFAULT_EARLIEST_VALID_EPOCH),
            "[00000012] "
        );
        assert_eq!(
            format_log_timestamp(1_704_067_200, false, DEFAULT_EARLIEST_VALID_EPOCH),
            "[2024-01-01T00:00:00Z]: "
        );
    }

    #[test]
    fn dict_log_epoch_timestamp_iso_format() {
        let dir = temp_dir("epoch-ts");
        let db_path = write_test_db(&dir, &[(0x1000, "time set\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1_704_067_200, 0x1000, &[], &[]);
        let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let lines = parser.feed(hex.as_bytes());

        assert!(
            lines.iter().any(|l| l.contains("[2024-01-01T00:00:00Z]: <inf> app: time set")),
            "{lines:?}"
        );
    }

    #[test]
    fn dict_log_parity_roundtrip_matches_plain_text() {
        let dir = temp_dir("parity-rt");
        let db_path = write_test_db(
            &dir,
            &[
                (0x1000, "boot ok\n"),
                (0x2000, "count=%d\n"),
            ],
        );
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg_a = encode_normal_message(3, 1, 10, 0x1000, &[], &[]);
        let msg_b = encode_normal_message(3, 1, 11, 0x2000, &[42, 0, 0, 0], &[]);
        let mut wire = Vec::new();
        wire.extend_from_slice(&msg_a);
        wire.extend_from_slice(&msg_b);
        let hex: String = wire.iter().map(|b| format!("{b:02x}")).collect();
        let lines = parser.feed(hex.as_bytes());

        assert!(lines.iter().any(|l| l.contains("<inf> app: boot ok")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("count=42")), "{lines:?}");
        assert!(
            !lines.iter().any(|l| l.chars().all(|c| c.is_ascii_hexdigit())),
            "raw hex in output: {lines:?}"
        );
    }

    #[test]
    fn dict_log_parity_real_reader_database_boot_prefix() {
        let Some(db) = std::env::var("EMBED_LOG_ZEPHYR_DICT_DATABASE")
            .ok()
            .filter(|path| std::path::Path::new(path).exists())
        else {
            eprintln!(
                "skip dict_log_parity_real_reader_database_boot_prefix: \
                 set EMBED_LOG_ZEPHYR_DICT_DATABASE and EMBED_LOG_ZEPHYR_DICT_FIXTURE"
            );
            return;
        };
        let Some(fixture) = std::env::var("EMBED_LOG_ZEPHYR_DICT_FIXTURE")
            .ok()
            .filter(|path| std::path::Path::new(path).exists())
        else {
            eprintln!("skip dict_log_parity_real_reader_database_boot_prefix: set EMBED_LOG_ZEPHYR_DICT_FIXTURE to a captured HEX log");
            return;
        };
        let hex = std::fs::read_to_string(&fixture)
            .expect("read EMBED_LOG_ZEPHYR_DICT_FIXTURE")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        let mut parser = ZephyrDictParser::new(&db);
        let lines = parser.feed(hex.as_bytes());

        assert!(
            !lines.is_empty(),
            "expected at least one decoded line from reader boot prefix, got none"
        );
        assert!(
            lines.iter().any(|l| l.contains("rv8263") || l.contains("<inf>")),
            "unexpected decode output: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.starts_with("003008")),
            "raw hex leaked: {lines:?}"
        );
    }

    #[test]
    fn missing_database_file_reports_error_once_then_drops_bytes() {
        let mut parser = ZephyrDictParser::new("/nonexistent/database.json");
        let first = parser.feed(b"000102");
        assert_eq!(first.len(), 1);
        assert!(first[0].contains("database not loaded"), "{}", first[0]);

        let second = parser.feed(b"000102");
        assert!(second.is_empty());
    }
}
