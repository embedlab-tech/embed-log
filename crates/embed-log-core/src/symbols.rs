//! Crash dump symbolization with the firmware ELF.
//!
//! A source can name its firmware ELF (`sources.<ID>.elf`). When a crash is
//! detected (see [`crate::events`]), the fault dump that follows is scanned for
//! code addresses (program counter, link register, call-trace return
//! addresses) and each address is resolved to `function (file:line)` with the
//! ELF's DWARF debug information, including inlined frames.
//!
//! The ELF is copied into the session as a readable source prefix plus a
//! SHA256 source-ID suffix, so each source keeps the binary that was running.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Maximum total lines of dump context, including the crash banner.
const MAX_DUMP_LINES: usize = 80;
/// Maximum time on either side of the crash banner for dump context.
const MAX_DUMP_MS: f64 = 5_000.0;
/// Directory inside a session with ELF snapshots.
pub const SNAPSHOT_DIR: &str = "symbols";

/// One code address found in a fault dump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DumpAddress {
    /// Where the address came from: `pc`, `lr`, `frame 2`, ...
    pub label: String,
    pub address: u64,
    /// Return addresses point after the call; they are looked up at `address - 1`.
    pub is_return: bool,
    /// The dump line it was found in.
    pub line: String,
}

/// One resolved frame. Inlined functions come first, the containing function last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Frame {
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub inlined: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecodedAddress {
    #[serde(flatten)]
    pub address: DumpAddress,
    /// Empty when the ELF has no code at this address (wrong ELF, or not a code address).
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrashReport {
    pub source: String,
    pub sequence: u64,
    /// The crash banner and bounded dump context before/after it (ANSI removed).
    pub lines: Vec<String>,
    pub addresses: Vec<DecodedAddress>,
    pub elf: String,
}

fn address_patterns() -> &'static [(Regex, &'static str, bool)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str, bool)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let re = |pattern: &str| Regex::new(pattern).expect("valid address regex");
        vec![
            // Cortex-M esf dump: `Faulting instruction address (r15/pc): 0x0000102a`
            (re(r"\(r15/pc\):\s*0x([0-9a-fA-F]+)"), "pc", false),
            // `r3/a4:  0x00000000 r12/ip:  0x00000000 r14/lr:  0x0000104b`
            (re(r"\br14/lr:\s*0x([0-9a-fA-F]+)"), "lr", true),
            // RISC-V: `mepc: 0x80000a10`, `ra: 0x80000b00` in the register dump.
            (re(r"\bmepc:\s*0x([0-9a-fA-F]+)"), "pc", false),
            // ARM64: `ELR_ELn: 0x...`
            (re(r"\bELR_EL[0-9]n?:\s*0x([0-9a-fA-F]+)"), "pc", false),
            // x86: `EIP: 0x...`
            (re(r"\bEIP:\s*0x([0-9a-fA-F]+)"), "pc", false),
        ]
    })
}

/// Call-trace rows (`CONFIG_EXCEPTION_STACK_TRACE`), after the log prefix:
/// `     0: fp: 0x80001234   ra: 0x80000abc` or `     1: sp: 0x... ra: 0x... [fn+0x12]`.
fn call_trace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(\d+):\s+(?:fp|sp):\s*0x[0-9a-fA-F]+\s+(?:ra|lr):\s*0x([0-9a-fA-F]+)")
            .expect("valid call trace regex")
    })
}

/// Text of a dump line without the log prefix noise that could hide matches.
fn dump_text(line: &str) -> String {
    crate::postprocess::strip_ansi(line)
}

/// Code addresses in fault dump lines, in dump order, without duplicates.
pub fn dump_addresses(lines: &[String]) -> Vec<DumpAddress> {
    let mut out: Vec<DumpAddress> = Vec::new();
    for raw in lines {
        let text = dump_text(raw);
        for (regex, label, is_return) in address_patterns() {
            if let Some(hex) = regex.captures(&text).and_then(|c| c.get(1)) {
                if let Ok(address) = u64::from_str_radix(hex.as_str(), 16) {
                    out.push(DumpAddress {
                        label: (*label).to_string(),
                        address,
                        is_return: *is_return,
                        line: text.trim().to_string(),
                    });
                }
            }
        }
        if let Some(captures) = call_trace_re().captures(&text) {
            if let Ok(address) = u64::from_str_radix(&captures[2], 16) {
                out.push(DumpAddress {
                    label: format!("frame {}", &captures[1]),
                    address,
                    is_return: true,
                    line: text.trim().to_string(),
                });
            }
        }
    }
    // A zero link register or return address is not a caller.
    out.retain(|address| address.address != 0);
    let mut seen = std::collections::HashSet::new();
    out.retain(|address| seen.insert((address.label.clone(), address.address)));
    out
}

/// DWARF lookups in one firmware ELF.
pub struct Symbolizer {
    loader: addr2line::Loader,
    /// ARM Thumb code: bit 0 of code addresses is the Thumb bit, not part of the address.
    thumb: bool,
}

impl Symbolizer {
    pub fn open(path: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if bytes.get(..4) != Some(b"\x7fELF") {
            return Err(format!("{} is not an ELF file", path.display()));
        }
        // e_machine at offset 18 (little or big endian); 40 = EM_ARM.
        let machine = match bytes.get(5) {
            Some(2) => bytes.get(18..20).map(|b| u16::from_be_bytes([b[0], b[1]])),
            _ => bytes.get(18..20).map(|b| u16::from_le_bytes([b[0], b[1]])),
        };
        let loader = addr2line::Loader::new(path)
            .map_err(|e| format!("cannot load debug information from {}: {e}", path.display()))?;
        Ok(Self {
            loader,
            thumb: machine == Some(40),
        })
    }

    /// Frames at `address`; return addresses are looked up inside the call.
    pub fn frames(&self, address: u64, is_return: bool) -> Vec<Frame> {
        let mut probe = if self.thumb { address & !1 } else { address };
        if is_return {
            probe = probe.saturating_sub(1);
        }
        let mut frames = Vec::new();
        if let Ok(mut iter) = self.loader.find_frames(probe) {
            while let Ok(Some(frame)) = iter.next() {
                let function = frame
                    .function
                    .as_ref()
                    .and_then(|name| name.demangle().ok())
                    .map(|name| name.into_owned());
                frames.push(Frame {
                    function,
                    file: frame
                        .location
                        .as_ref()
                        .and_then(|l| l.file)
                        .map(str::to_string),
                    line: frame.location.as_ref().and_then(|l| l.line),
                    inlined: false,
                });
            }
        }
        if frames.iter().all(|frame| frame.function.is_none()) {
            // Without DWARF function info, fall back to the symbol table, but
            // only inside code with line info (the symbol table lookup alone
            // also "finds" addresses past the end of the firmware).
            let location = self.loader.find_location(probe).ok().flatten();
            if let Some(symbol) = location.as_ref().and(self.loader.find_symbol(probe)) {
                frames = vec![Frame {
                    function: Some(addr2line::demangle_auto(symbol.into(), None).into_owned()),
                    file: location.as_ref().and_then(|l| l.file).map(str::to_string),
                    line: location.as_ref().and_then(|l| l.line),
                    inlined: false,
                }];
            }
        }
        // All but the last (outermost) frame are inlined into it.
        let count = frames.len();
        for frame in frames.iter_mut().take(count.saturating_sub(1)) {
            frame.inlined = true;
        }
        frames
    }

    pub fn decode(&self, addresses: Vec<DumpAddress>) -> Vec<DecodedAddress> {
        addresses
            .into_iter()
            .map(|address| DecodedAddress {
                frames: self.frames(address.address, address.is_return),
                address,
            })
            .collect()
    }
}

#[derive(serde::Deserialize)]
struct JournalRecord {
    source_id: String,
    sequence: u64,
    message: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default, rename = "absNum")]
    abs_num: Option<f64>,
}

/// The crash at `sequence`, including a bounded register/trace prefix before
/// its banner. Zephyr software panic/oops dumps registers before the fatal
/// banner, whereas hardware faults commonly have a FAULT banner first.
/// Context never crosses another event, source, 80 lines or 5 seconds.
pub fn crash_dump_lines(
    journal: &Path,
    source: &str,
    sequence: u64,
) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(journal)
        .map_err(|e| format!("cannot open {}: {e}", journal.display()))?;
    let needle = format!(
        "\"source_id\":{}",
        serde_json::to_string(source).unwrap_or_default()
    );
    let mut preceding: VecDeque<JournalRecord> = VecDeque::new();
    let mut lines = Vec::new();
    let mut started_at: Option<f64> = None;
    let mut found = false;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        if !line.contains(&needle) {
            continue;
        }
        let Ok(record) = serde_json::from_str::<JournalRecord>(&line) else {
            continue;
        };
        if record.source_id != source || record.kind.as_deref() == Some("tx") {
            continue;
        }
        if record.sequence < sequence {
            // A previous restart/crash is a hard boundary for lookbehind.
            if record.event.is_some() {
                preceding.clear();
            } else {
                preceding.push_back(record);
                if preceding.len() >= MAX_DUMP_LINES {
                    preceding.pop_front();
                }
            }
            continue;
        }
        if !found {
            if record.sequence != sequence {
                return Err(format!(
                    "record {sequence} of {source} is not in the session"
                ));
            }
            if record.event.as_deref() != Some(crate::events::CRASH) {
                return Err(format!(
                    "record {sequence} of {source} is not a detected crash"
                ));
            }
            found = true;
            started_at = record.abs_num;
            preceding.retain(|prior| match (started_at, prior.abs_num) {
                (Some(now), Some(before)) => (0.0..=MAX_DUMP_MS).contains(&(now - before)),
                _ => true,
            });
            // Do not attach unrelated ordinary logs before a hardware FAULT
            // banner. Only retain a prefix that actually contains code addresses.
            if let Some(first) = preceding
                .iter()
                .position(|prior| !dump_addresses(std::slice::from_ref(&prior.message)).is_empty())
            {
                lines.extend(
                    preceding
                        .iter()
                        .skip(first)
                        .map(|prior| dump_text(&prior.message)),
                );
            }
        } else {
            let too_late = matches!((started_at, record.abs_num), (Some(start), Some(now)) if now - start > MAX_DUMP_MS);
            if record.event.is_some() || too_late || lines.len() >= MAX_DUMP_LINES {
                break;
            }
        }
        lines.push(dump_text(&record.message));
    }
    if !found {
        return Err(format!(
            "record {sequence} of {source} is not in the session"
        ));
    }
    Ok(lines)
}

/// Decode the crash at `sequence` of `source` from a session journal.
pub fn decode_crash(
    journal: &Path,
    source: &str,
    sequence: u64,
    elf: &Path,
) -> Result<CrashReport, String> {
    let lines = crash_dump_lines(journal, source, sequence)?;
    let symbolizer = Symbolizer::open(elf)?;
    let addresses = symbolizer.decode(dump_addresses(&lines));
    Ok(CrashReport {
        source: source.to_string(),
        sequence,
        lines,
        addresses,
        elf: elf.display().to_string(),
    })
}

/// Snapshot path of a source's ELF inside a session directory.
pub fn snapshot_path(session_dir: &Path, source: &str) -> PathBuf {
    let safe: String = source
        .chars()
        .take(40)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // The readable prefix alone is lossy (MCU.A and MCU_A), including on
    // case-insensitive filesystems. Hash the exact source ID for every source.
    let digest = Sha256::digest(source.as_bytes());
    session_dir
        .join(SNAPSHOT_DIR)
        .join(format!("{safe}-{digest:x}.elf"))
}

/// Copy configured ELF files into a new session. Returns `{source: snapshot}`
/// for the manifest; a source whose ELF cannot be copied is left out (and
/// logged), it then cannot be decoded in this session.
pub fn snapshot_elfs(session_dir: &Path, elfs: &[(String, PathBuf)]) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (source, elf) in elfs {
        let target = snapshot_path(session_dir, source);
        let copied = target
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()
            .and_then(|_| std::fs::copy(elf, &target));
        match copied {
            Ok(_) => {
                let relative = target.strip_prefix(session_dir).unwrap_or(&target);
                out.insert(
                    source.clone(),
                    serde_json::json!(relative.to_string_lossy().replace('\\', "/")),
                );
            }
            Err(error) => {
                tracing::warn!("[{source}] cannot snapshot ELF {}: {error}", elf.display())
            }
        }
    }
    serde_json::Value::Object(out)
}

/// The ELF snapshot of `source` recorded in a session manifest (`symbols`).
pub fn manifest_elf(
    session_dir: &Path,
    manifest: &serde_json::Value,
    source: &str,
) -> Result<PathBuf, String> {
    let relative = manifest
        .get("symbols")
        .and_then(|symbols| symbols.get(source))
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("no firmware ELF is configured for {source} in this session"))?;
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid ELF path in the session manifest".into());
    }
    Ok(session_dir.join(relative))
}

/// Human-readable report: the dump's first line, then each address with its frames.
pub fn report_text(report: &CrashReport) -> String {
    let mut out = format!(
        "Crash #{} on {}  (ELF: {})\n",
        report.sequence, report.source, report.elf
    );
    if let Some(first) = report.lines.first() {
        out.push_str(&format!("  {first}\n"));
    }
    if report.addresses.is_empty() {
        out.push_str("  no code addresses found in the fault dump\n");
    }
    for decoded in &report.addresses {
        let head = format!(
            "  {:<8} 0x{:08x}",
            decoded.address.label, decoded.address.address
        );
        if decoded.frames.is_empty() {
            out.push_str(&format!("{head}  ?? (not in this ELF)\n"));
            continue;
        }
        for (i, frame) in decoded.frames.iter().enumerate() {
            let prefix = if i == 0 {
                head.clone()
            } else {
                " ".repeat(head.len())
            };
            let place = match (&frame.file, frame.line) {
                (Some(file), Some(line)) => format!("{file}:{line}"),
                (Some(file), None) => file.clone(),
                _ => "??".into(),
            };
            out.push_str(&format!(
                "{prefix}  {}  {place}{}\n",
                frame.function.as_deref().unwrap_or("??"),
                if frame.inlined { "  (inlined)" } else { "" }
            ));
        }
    }
    out
}

/// Validate a configured ELF path (exists, ELF with debug information).
pub fn validate_elf(path: &Path, ctx: &str) -> Result<(), String> {
    Symbolizer::open(path)
        .map(|_| ())
        .map_err(|e| format!("{ctx}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-m4-fault.elf")
    }

    fn dump() -> Vec<String> {
        [
            "[00:00:05.100,000] <err> os: ***** MPU FAULT *****",
            "[00:00:05.100,000] <err> os:   Data Access Violation",
            "[00:00:05.100,000] <err> os:   MMFAR Address: 0x0",
            "[00:00:05.100,000] <err> os: r0/a1:  0x00000000  r1/a2:  0x00000000  r2/a3:  0x00000000",
            "[00:00:05.100,000] <err> os: r3/a4:  0x00000000 r12/ip:  0x00000000 r14/lr:  0x0000104b",
            "[00:00:05.100,000] <err> os:  xpsr:  0x61000000",
            "[00:00:05.100,000] <err> os: Faulting instruction address (r15/pc): 0x0000102a",
            "[00:00:05.101,000] <err> os: >>> ZEPHYR FATAL ERROR 19: Unknown error on CPU 0",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn finds_pc_lr_and_call_trace_addresses() {
        let found = dump_addresses(&dump());
        let labels: Vec<_> = found
            .iter()
            .map(|a| (a.label.as_str(), a.address, a.is_return))
            .collect();
        assert_eq!(labels, vec![("lr", 0x104b, true), ("pc", 0x102a, false)]);
        let trace = dump_addresses(&[
            "\x1b[1;31m<err> os: call trace:\x1b[0m".to_string(),
            "<err> os:      0: fp: 0x80001234   ra: 0x80000abc".to_string(),
            "<err> os:      1: fp: 0x80001240   ra: 0x00000000".to_string(),
            "<err> os: mepc: 0x80000a10".to_string(),
        ]);
        let labels: Vec<_> = trace
            .iter()
            .map(|a| (a.label.as_str(), a.address))
            .collect();
        assert_eq!(labels, vec![("frame 0", 0x80000abc), ("pc", 0x80000a10)]);
    }

    #[test]
    fn resolves_inlined_frames_and_callers_in_a_cortex_m_elf() {
        let symbolizer = Symbolizer::open(&fixture()).unwrap();
        let decoded = symbolizer.decode(dump_addresses(&dump()));
        let pc = decoded.iter().find(|d| d.address.label == "pc").unwrap();
        let functions: Vec<_> = pc
            .frames
            .iter()
            .map(|f| (f.function.as_deref().unwrap(), f.inlined))
            .collect();
        assert_eq!(
            functions,
            vec![("scale_sample", true), ("process_sample", false)]
        );
        assert_eq!(
            pc.frames[0].file.as_deref(),
            Some("/work/firmware/src/sensor.c")
        );
        assert_eq!(pc.frames[0].line, Some(7), "the faulting dereference");
        let lr = decoded.iter().find(|d| d.address.label == "lr").unwrap();
        assert_eq!(
            lr.frames.last().unwrap().function.as_deref(),
            Some("sensor_task")
        );
        assert_eq!(
            lr.frames.last().unwrap().line,
            Some(20),
            "the call site of process_sample"
        );
        // An address outside the firmware resolves to nothing.
        assert!(symbolizer.frames(0x0800_0000, false).is_empty());
    }

    #[test]
    fn decodes_registers_before_a_software_fatal_banner() {
        let dir = std::env::temp_dir().join(format!("embed-log-pre-fatal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("combined.jsonl");
        let rows = [
            serde_json::json!({"source_id":"DUT", "sequence":1, "message":"ordinary log", "absNum":1000}),
            serde_json::json!({"source_id":"DUT", "sequence":2, "message":"r14/lr: 0x0000104b", "absNum":1001}),
            serde_json::json!({"source_id":"NET", "sequence":3, "message":"r14/lr: 0xdeadbeef", "absNum":1001}),
            serde_json::json!({"source_id":"DUT", "sequence":4, "message":"Faulting instruction address (r15/pc): 0x0000102a", "absNum":1002}),
            serde_json::json!({"source_id":"DUT", "sequence":5, "message":">>> ZEPHYR FATAL ERROR 4: Kernel panic on CPU 0", "event":"crash", "absNum":1003}),
            serde_json::json!({"source_id":"DUT", "sequence":6, "message":"0: fp: 0x20001000 ra: 0x0000104b", "absNum":1004}),
            serde_json::json!({"source_id":"DUT", "sequence":7, "message":"*** Booting Zephyr OS ***", "event":"boot", "absNum":1005}),
        ];
        std::fs::write(
            &journal,
            rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
        let report = decode_crash(&journal, "DUT", 5, &fixture()).unwrap();
        let labels: Vec<_> = report
            .addresses
            .iter()
            .map(|a| a.address.label.as_str())
            .collect();
        assert_eq!(labels, vec!["lr", "pc", "frame 0"]);
        assert_eq!(
            report.addresses[1].frames[0].function.as_deref(),
            Some("scale_sample")
        );
        assert_eq!(
            report.addresses[0]
                .frames
                .last()
                .unwrap()
                .function
                .as_deref(),
            Some("sensor_task")
        );
        assert_eq!(report.lines.len(), 4);
        assert!(!report
            .lines
            .iter()
            .any(|l| l.contains("ordinary") || l.contains("deadbeef") || l.contains("Booting")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pre_fatal_context_is_bounded_by_time_events_and_line_count() {
        let dir =
            std::env::temp_dir().join(format!("embed-log-dump-bounds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("combined.jsonl");
        for boundary in ["time", "boot", "crash", "lines"] {
            let mut rows = vec![
                serde_json::json!({"source_id":"DUT", "sequence":1, "message":"r14/lr: 0x0000104b", "absNum":0}),
            ];
            let boundary_event = match boundary {
                "boot" => Some("boot"),
                "crash" => Some("crash"),
                _ => None,
            };
            if let Some(event) = boundary_event {
                rows.push(serde_json::json!({"source_id":"DUT", "sequence":2, "message":"boundary", "event":event, "absNum":1}));
            }
            if boundary == "lines" {
                for sequence in 2..=100 {
                    rows.push(serde_json::json!({"source_id":"DUT", "sequence":sequence, "message":"register context", "absNum":1}));
                }
            }
            rows.push(serde_json::json!({"source_id":"DUT", "sequence":101, "message":">>> ZEPHYR FATAL ERROR 4", "event":"crash", "absNum":if boundary == "time" {6000} else {2}}));
            std::fs::write(
                &journal,
                rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
            )
            .unwrap();
            let lines = crash_dump_lines(&journal, "DUT", 101).unwrap();
            assert_eq!(lines, vec![">>> ZEPHYR FATAL ERROR 4"], "{boundary}");
        }
        // With no timestamps, the line bound still applies to older journals.
        let mut rows: Vec<_> = (1..=200).map(|sequence| serde_json::json!({"source_id":"DUT", "sequence":sequence, "message":"r14/lr: 0x0000104b"})).collect();
        rows.push(serde_json::json!({"source_id":"DUT", "sequence":201, "message":">>> ZEPHYR FATAL ERROR 4", "event":"crash"}));
        std::fs::write(
            &journal,
            rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
        assert_eq!(
            crash_dump_lines(&journal, "DUT", 201).unwrap().len(),
            MAX_DUMP_LINES
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn colliding_source_labels_keep_distinct_elf_snapshots() {
        let dir =
            std::env::temp_dir().join(format!("embed-log-elf-collision-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = std::fs::read(fixture()).unwrap();
        let mut second = first.clone();
        second.extend_from_slice(b"second firmware");
        let other = dir.join("other.elf");
        std::fs::write(&other, &second).unwrap();
        let snapshots = snapshot_elfs(
            &dir,
            &[("MCU.A".into(), fixture()), ("MCU_A".into(), other)],
        );
        let manifest = serde_json::json!({"symbols":snapshots});
        let a = manifest_elf(&dir, &manifest, "MCU.A").unwrap();
        let b = manifest_elf(&dir, &manifest, "MCU_A").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(a).unwrap(), first);
        assert_eq!(std::fs::read(b).unwrap(), second);
        assert_ne!(
            snapshot_path(&dir, "MCU").to_string_lossy().to_lowercase(),
            snapshot_path(&dir, "mcu").to_string_lossy().to_lowercase()
        );
        // A manifest made by older versions remains decodable.
        let legacy = serde_json::json!({"symbols":{"DUT":"symbols/DUT.elf"}});
        assert_eq!(
            manifest_elf(&dir, &legacy, "DUT").unwrap(),
            dir.join("symbols/DUT.elf")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_files_that_are_not_elf() {
        let path = std::env::temp_dir().join(format!("embed-log-not-elf-{}", std::process::id()));
        std::fs::write(&path, b"not an elf").unwrap();
        assert!(validate_elf(&path, "sources.DUT.elf")
            .unwrap_err()
            .contains("not an ELF"));
        assert!(
            validate_elf(Path::new("/nonexistent.elf"), "sources.DUT.elf")
                .unwrap_err()
                .starts_with("sources.DUT.elf: cannot read")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn decodes_a_crash_from_a_session_journal() {
        let dir = std::env::temp_dir().join(format!("embed-log-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("combined.jsonl");
        let mut text = String::new();
        let mut sequence = 1;
        let mut push = |source: &str, message: &str, event: Option<&str>, ms: f64| {
            let mut record = serde_json::json!({"source_id": source, "sequence": sequence, "message": message, "type": "rx", "absNum": 1_700_000_000_000.0 + ms});
            if let Some(event) = event {
                record["event"] = serde_json::json!(event);
            }
            text.push_str(&record.to_string());
            text.push('\n');
            sequence += 1;
        };
        push("DUT", "<inf> main: ready", None, 0.0);
        let lines = dump();
        push("DUT", &lines[0], Some("crash"), 100.0);
        push("NET", "<inf> net: unrelated", None, 101.0);
        for line in &lines[1..] {
            push("DUT", line, None, 102.0);
        }
        push("DUT", "*** Booting Zephyr OS ***", Some("boot"), 200.0);
        push("DUT", "r14/lr:  0x00001067 after reboot", None, 201.0);
        std::fs::write(&journal, text).unwrap();

        let report = decode_crash(&journal, "DUT", 2, &fixture()).unwrap();
        assert_eq!(report.lines.len(), 8, "{:?}", report.lines);
        assert!(report
            .lines
            .iter()
            .all(|l| !l.contains("unrelated") && !l.contains("Booting")));
        let labels: Vec<_> = report
            .addresses
            .iter()
            .map(|a| a.address.label.as_str())
            .collect();
        assert_eq!(labels, vec!["lr", "pc"]);
        assert!(
            decode_crash(&journal, "DUT", 3, &fixture()).is_err(),
            "3 is a NET record"
        );

        let snapshot = snapshot_elfs(
            &dir,
            &[
                ("DUT".into(), fixture()),
                ("BAD".into(), dir.join("missing.elf")),
            ],
        );
        let target = snapshot_path(&dir, "DUT");
        assert_eq!(
            snapshot["DUT"],
            target
                .strip_prefix(&dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        assert!(target.is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
