//! Device boot and crash detection.
//!
//! Received console lines are matched against Zephyr's boot banner and
//! fatal-error output. A match marks the record with `event: "boot"` or
//! `event: "crash"` (live payload, replay, `combined.jsonl` and HTML exports),
//! and the session keeps per-source counts.
//!
//! One crash prints several matching lines (fault banner, `>>> ZEPHYR FATAL
//! ERROR`, assertion text) and a reset through MCUboot prints two boot
//! banners, so matches within `GROUP_WINDOW` of the previous one of the same
//! kind are one event. A boot ends a crash group; a crash ends a boot group.

use std::sync::OnceLock;

use chrono::{DateTime, Duration, Local};
use regex::{Regex, RegexSet};

pub const BOOT: &str = "boot";
pub const CRASH: &str = "crash";

/// Matches of the same kind closer together than this are one event.
const GROUP_WINDOW_MS: i64 = 5_000;

fn boot_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `*** Booting Zephyr OS build v3.7.0 ***`, `*** Booting nRF Connect SDK v2.7.0 ***`,
    // `*** Booting MCUboot v2.1.0 ***`.
    RE.get_or_init(|| Regex::new(r"\*\*\* Booting \S").expect("valid boot regex"))
}

fn crash_set() -> &'static RegexSet {
    static SET: OnceLock<RegexSet> = OnceLock::new();
    SET.get_or_init(|| {
        RegexSet::new([
            // z_fatal_error(): `>>> ZEPHYR FATAL ERROR 3: Kernel oops on CPU 0`
            r">>> ZEPHYR FATAL ERROR",
            // __ASSERT(): `ASSERTION FAIL [x == 1] @ WEST_TOPDIR/...`
            r"ASSERTION FAIL",
            // Cortex-M fault banners: `***** HARD FAULT *****`, `***** MPU FAULT *****`,
            // `***** BUS FAULT *****`, `***** USAGE FAULT *****`, `***** SECURE FAULT *****`.
            r"\*{5} [A-Z ]*FAULT \*{5}",
            // Other architectures and older kernels.
            r"Fatal fault in (?:ISR|thread)",
            r"\bKernel (?:panic|oops)\b",
        ])
        .expect("valid crash regexes")
    })
}

/// Stateful per-source detector (grouping looks at earlier lines).
#[derive(Debug, Default)]
pub struct EventDetector {
    last_boot: Option<DateTime<Local>>,
    last_crash: Option<DateTime<Local>>,
}

impl EventDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Classify one received line (not TX echoes) logged at `at`.
    pub fn detect(&mut self, message: &str, at: DateTime<Local>) -> Option<&'static str> {
        let stripped;
        let text = if message.contains('\x1b') {
            stripped = crate::postprocess::strip_ansi(message);
            stripped.as_str()
        } else {
            message
        };
        let window = Duration::milliseconds(GROUP_WINDOW_MS);
        let recent = |last: Option<DateTime<Local>>| last.is_some_and(|last| at - last < window);
        if boot_re().is_match(text) {
            let new_boot = !recent(self.last_boot);
            self.last_boot = Some(at);
            self.last_crash = None;
            return new_boot.then_some(BOOT);
        }
        if crash_set().is_match(text) {
            let new_crash = !recent(self.last_crash);
            self.last_crash = Some(at);
            self.last_boot = None;
            return new_crash.then_some(CRASH);
        }
        None
    }
}

/// Per-source event counts kept by the session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct EventCounts {
    pub boot: u64,
    pub crash: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: i64) -> DateTime<Local> {
        DateTime::from_timestamp_millis(1_700_000_000_000 + ms)
            .unwrap()
            .with_timezone(&Local)
    }

    #[test]
    fn detects_zephyr_boot_banners_and_groups_bootloader_and_app() {
        let mut d = EventDetector::new();
        assert_eq!(
            d.detect("*** Booting MCUboot v2.1.0 ***", at(0)),
            Some(BOOT)
        );
        assert_eq!(
            d.detect("*** Booting nRF Connect SDK v2.7.0 ***", at(300)),
            None
        );
        assert_eq!(d.detect("*** Using Zephyr OS v3.6.99 ***", at(301)), None);
        assert_eq!(
            d.detect("[00:00:00.000,000] <inf> main: ready", at(400)),
            None
        );
        assert_eq!(
            d.detect("\x1b[0m*** Booting Zephyr OS build v3.7.0 ***", at(60_000)),
            Some(BOOT)
        );
    }

    #[test]
    fn one_crash_dump_is_one_crash_and_a_reboot_starts_a_new_group() {
        let mut d = EventDetector::new();
        let dump = [
            "[00:00:05.100,000] <err> os: ***** MPU FAULT *****",
            "[00:00:05.100,000] <err> os:   Data Access Violation",
            "[00:00:05.101,000] <err> os: >>> ZEPHYR FATAL ERROR 19: Unknown error on CPU 0",
            "[00:00:05.101,000] <err> os: Current thread: 0x20000a40 (main)",
        ];
        let got: Vec<_> = dump
            .iter()
            .enumerate()
            .map(|(i, l)| d.detect(l, at(i as i64)))
            .collect();
        assert_eq!(got, vec![Some(CRASH), None, None, None]);
        assert_eq!(
            d.detect("*** Booting Zephyr OS build v3.7.0 ***", at(1_000)),
            Some(BOOT)
        );
        // A crash right after the reboot is a new crash, not part of the old dump.
        assert_eq!(
            d.detect("ASSERTION FAIL [x == 1] @ src/main.c:12", at(1_500)),
            Some(CRASH)
        );
        assert_eq!(
            d.detect(">>> ZEPHYR FATAL ERROR 4: Kernel panic on CPU 0", at(1_501)),
            None
        );
        assert_eq!(
            d.detect("ASSERTION FAIL [y] @ src/main.c:40", at(30_000)),
            Some(CRASH)
        );
    }

    #[test]
    fn fast_crash_reboot_cycles_keep_each_boot_boundary() {
        let mut d = EventDetector::new();
        for cycle in 0..3 {
            let ms = cycle * 1000;
            assert_eq!(d.detect("*** Booting MCUboot ***", at(ms)), Some(BOOT));
            assert_eq!(d.detect("*** Booting Zephyr OS ***", at(ms + 100)), None);
            assert_eq!(d.detect("ASSERTION FAIL [x]", at(ms + 200)), Some(CRASH));
            assert_eq!(d.detect(">>> ZEPHYR FATAL ERROR 4", at(ms + 300)), None);
        }
    }

    #[test]
    fn ordinary_lines_mentioning_faults_are_not_crashes() {
        let mut d = EventDetector::new();
        for line in [
            "<inf> app: no fault detected",
            "<wrn> app: retrying after BUS FAULT counter reset",
            "booting sensor",
            "<inf> shell: kernel panic test skipped",
        ] {
            assert_eq!(d.detect(line, at(0)), None, "{line}");
        }
        assert_eq!(
            d.detect("Fatal fault in thread 0x20001000! Aborting.", at(0)),
            Some(CRASH)
        );
        assert_eq!(d.detect("***** HARD FAULT *****", at(10_000)), Some(CRASH));
    }
}
