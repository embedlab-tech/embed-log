//! Zephyr dictionary-logging parser.
//!
//! Ports the wire format understood by Zephyr's
//! `scripts/logging/dictionary` Python tools (`log_parser_v3.py` /
//! `log_database.py`): a binary stream of self-length-prefixed messages,
//! decoded against a `database.json` built at compile time (via
//! `database_gen.py` from the firmware ELF) that maps format-string/log
//! source pointers back to human-readable text.
//!
//! Scope: database format version 3 only (the current Zephyr default —
//! `LogDatabase.ZEPHYR_DICT_LOG_VER` — since 2022). Versions 1/2 use a
//! different bit-packed header and MIPI Sys-T output is a different backend
//! entirely; both are skipped here. Add if a real project still needs them.

use std::collections::HashMap;

use serde::Deserialize;

use super::traits::StreamParser;

const MSG_TYPE_NORMAL: u8 = 0;
const MSG_TYPE_DROPPED: u8 = 1;
const LOG_HEX_SEP: &str = "##ZLOGV1##";

/// Dictionary wire format on the serial port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    Binary,
    Hex,
}

impl WireFormat {
    pub fn from_config(value: Option<&str>) -> Self {
        match value.unwrap_or("binary") {
            "hex" => Self::Hex,
            _ => Self::Binary,
        }
    }
}

/// Zephyr dictionary-logging parser for a raw byte stream (UART/UDP/file).
///
/// Buffers incomplete messages across `feed()` calls: each message declares
/// its own length in its header, so — unlike SLIP — there's no delimiter
/// byte to resync on. On an unrecognized message type (framing desync) the
/// buffer is dropped and decoding resumes from the next `feed()` call.
///
/// When [`WireFormat::Hex`] is selected, incoming UART bytes are treated as
/// ASCII hex (optionally prefixed with `##ZLOGV1##`) before binary parsing.
pub struct ZephyrDictParser {
    state: LoadState,
    wire_format: WireFormat,
    buf: Vec<u8>,
    hex_buf: Vec<u8>,
    decoded_line_count: usize,
    reported_load_error: bool,
}

enum LoadState {
    Ready(Database),
    Failed(String),
}

impl ZephyrDictParser {
    pub fn new(database_path: &str) -> Self {
        Self::with_options(database_path, WireFormat::Binary, false)
    }

    pub fn with_options(database_path: &str, wire_format: WireFormat, gwl: bool) -> Self {
        let state = match Database::load(database_path, gwl) {
            Ok(db) => LoadState::Ready(db),
            Err(e) => {
                tracing::error!("zephyr-dict: failed to load database {database_path:?}: {e}");
                LoadState::Failed(e)
            }
        };
        Self {
            state,
            wire_format,
            buf: Vec::new(),
            hex_buf: Vec::new(),
            decoded_line_count: 0,
            reported_load_error: false,
        }
    }

    fn refresh_binary_from_hex(&mut self) {
        if self.hex_buf.len() > 512 * 1024 {
            if let Some(idx) = find_subslice(&self.hex_buf, LOG_HEX_SEP.as_bytes()) {
                self.hex_buf.drain(0..idx);
            } else {
                let keep = self.hex_buf.len().saturating_sub(256 * 1024);
                self.hex_buf.drain(0..keep);
            }
            self.decoded_line_count = 0;
        }
        self.buf = extract_hex_logdata(&self.hex_buf);
    }

    fn decode_all_messages(&self, db: &Database) -> Vec<String> {
        let mut buf = self.buf.clone();
        let mut lines = Vec::new();
        loop {
            match take_one_message(&buf, db) {
                TakeResult::NeedMore => break,
                TakeResult::Consumed { len, mut output } => {
                    buf.drain(0..len);
                    lines.append(&mut output);
                }
                TakeResult::Stop => break,
            }
        }
        lines
    }
}

impl StreamParser for ZephyrDictParser {
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        let db = match &self.state {
            LoadState::Ready(db) => db,
            LoadState::Failed(err) => {
                if self.reported_load_error {
                    return Vec::new();
                }
                self.reported_load_error = true;
                return vec![format!(
                    "[zephyr-dict: database not loaded ({err}); check parser.database — dropping incoming bytes]"
                )];
            }
        };

        if self.wire_format == WireFormat::Hex {
            self.hex_buf.extend_from_slice(data);
            self.refresh_binary_from_hex();
            let all_lines = match &self.state {
                LoadState::Ready(db) => self.decode_all_messages(db),
                LoadState::Failed(_) => return Vec::new(),
            };
            let new_lines = if self.decoded_line_count < all_lines.len() {
                all_lines[self.decoded_line_count..].to_vec()
            } else {
                Vec::new()
            };
            self.decoded_line_count = all_lines.len();
            return new_lines;
        }

        self.buf.extend_from_slice(data);
        let mut lines = Vec::new();

        loop {
            match take_one_message(&self.buf, db) {
                TakeResult::NeedMore => break,
                TakeResult::Consumed { len, mut output } => {
                    self.buf.drain(0..len);
                    lines.append(&mut output);
                }
                TakeResult::Stop => break,
            }
        }

        lines
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn unhexify_chunk(hexstr: &str) -> Option<Vec<u8>> {
    if hexstr.len() < 20 {
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

fn extract_hex_runs(text: &str) -> Vec<u8> {
    let mut parts: Vec<u8> = Vec::new();
    let mut idx = 0;
    let bytes = text.as_bytes();
    while idx < bytes.len() {
        let ch = bytes[idx] as char;
        if !ch.is_ascii_hexdigit() {
            idx += 1;
            continue;
        }
        let start = idx;
        while idx < bytes.len() && (bytes[idx] as char).is_ascii_hexdigit() {
            idx += 1;
        }
        if let Some(packet) = unhexify_chunk(std::str::from_utf8(&bytes[start..idx]).unwrap_or("")) {
            parts.extend(packet);
        }
    }
    parts
}

fn extract_hex_logdata(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let mut parts: Vec<u8> = Vec::new();
    let mut search_from = 0usize;

    while let Some(rel) = text[search_from..].find(LOG_HEX_SEP) {
        let sep_start = search_from + rel;
        let mut idx = sep_start + LOG_HEX_SEP.len();
        let mut hexchars = String::new();
        while idx < text.len() {
            let ch = text.as_bytes()[idx] as char;
            if ch.is_ascii_hexdigit() {
                hexchars.push(ch);
                idx += 1;
            } else if matches!(ch, '\r' | '\n' | '\t' | ' ') {
                idx += 1;
            } else {
                break;
            }
        }
        if !hexchars.is_empty() {
            if let Some(packet) = unhexify_chunk(&hexchars) {
                parts.extend(packet);
            }
        }
        search_from = idx;
    }

    if !parts.is_empty() {
        return parts;
    }

    extract_hex_runs(&text)
}

fn formalize_gwl_fmt_string(fmt: &str) -> String {
    let mut s = fmt.to_string();
    for (src, dst) in [
        ("%zu", "%u"),
        ("%zd", "%d"),
        ("%zi", "%d"),
        ("%PRIu32", "%u"),
        ("%PRId32", "%d"),
        ("%PRIx32", "%x"),
    ] {
        s = s.replace(src, dst);
    }
    s
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
        let prefix = format!(
            "[{timestamp:>10}] <{}> {source_name}: ",
            level_name(level)
        );
        push_display_lines(&mut output, Some(&prefix), &body);
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

    fn load(path: &str, gwl: bool) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path:?}: {e}"))?;
        let raw: RawDatabase =
            serde_json::from_str(&text).map_err(|e| format!("parse {path:?}: {e}"))?;

        if raw.version != 3 {
            return Err(format!(
                "unsupported database version {} (only version 3 is supported)",
                raw.version
            ));
        }

        let mut string_mappings = HashMap::with_capacity(raw.string_mappings.len());
        for (addr, s) in raw.string_mappings {
            if let Ok(addr) = addr.parse::<u64>() {
                let value = if gwl {
                    formalize_gwl_fmt_string(&s)
                } else {
                    s
                };
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

        Ok(Self {
            is_64bit: raw.target.bits.unwrap_or(32) == 64,
            little_endian: raw.target.little_endianness.unwrap_or(true),
            arch: raw.arch,
            timestamp_64bit: raw.kconfigs.contains_key("CONFIG_LOG_TIMESTAMP_64BIT"),
            log_instances,
            string_mappings,
            sections,
        })
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

    #[test]
    fn decodes_simple_string_only_message() {
        let dir = temp_dir("simple");
        // fmt string lives at address 0x1000
        let db_path = write_test_db(&dir, &[(0x1000, "hello world\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 42, 0x1000, &[], &[]);
        let lines = parser.feed(&msg);

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

        let lines = parser.feed(&msg);
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

        let lines = parser.feed(&msg);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("name=widget"), "{}", lines[0]);
    }

    #[test]
    fn level_none_message_has_no_prefix() {
        let dir = temp_dir("none-level");
        let db_path = write_test_db(&dir, &[(0x1000, "raw printk\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(0, 1, 1, 0x1000, &[], &[]);
        let lines = parser.feed(&msg);
        assert_eq!(lines, vec!["raw printk"]);
    }

    #[test]
    fn dropped_message_reports_count() {
        let dir = temp_dir("dropped");
        let db_path = write_test_db(&dir, &[]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let mut msg = vec![MSG_TYPE_DROPPED];
        msg.extend_from_slice(&7u16.to_le_bytes());
        let lines = parser.feed(&msg);
        assert_eq!(lines, vec!["--- 7 messages dropped ---"]);
    }

    #[test]
    fn reassembles_message_split_across_feed_calls() {
        let dir = temp_dir("split");
        let db_path = write_test_db(&dir, &[(0x1000, "hello\n")]);
        let mut parser = ZephyrDictParser::new(&db_path);

        let msg = encode_normal_message(3, 1, 1, 0x1000, &[], &[]);
        let (a, b) = msg.split_at(msg.len() / 2);

        assert!(parser.feed(a).is_empty());
        let lines = parser.feed(b);
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
        let lines = parser.feed(&msg);

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
        let lines = parser.feed(&buf);
        assert!(lines.is_empty(), "expected stop before any decoded line, got: {lines:?}");
    }

    #[test]
    fn decodes_hex_wire_format_message() {
        let dir = temp_dir("hexwire");
        let db_path = write_test_db(&dir, &[(0x1000, "hello world\n")]);
        let mut parser = ZephyrDictParser::with_options(&db_path, WireFormat::Hex, false);

        let msg = encode_normal_message(3, 1, 42, 0x1000, &[], &[]);
        let hex_body: String = msg.iter().map(|b| format!("{b:02x}")).collect();
        let hex_line = format!("{LOG_HEX_SEP}\r\n{hex_body}");
        let lines = parser.feed(hex_line.as_bytes());

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("<inf> app: hello world"), "{}", lines[0]);
    }

    #[test]
    fn missing_database_file_reports_error_once_then_drops_bytes() {
        let mut parser = ZephyrDictParser::new("/nonexistent/database.json");
        let first = parser.feed(b"\x00\x01\x02");
        assert_eq!(first.len(), 1);
        assert!(first[0].contains("database not loaded"), "{}", first[0]);

        let second = parser.feed(b"\x00\x01\x02");
        assert!(second.is_empty());
    }
}
