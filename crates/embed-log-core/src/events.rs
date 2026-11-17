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

use crate::config::SourceEvents;

pub const BOOT: &str = "boot";
pub const CRASH: &str = "crash";

/// Matches of the same kind closer together than this are one event.
const GROUP_WINDOW_MS: i64 = 5_000;

const BUILTIN_BOOT: &[&str] = &[
    // `*** Booting Zephyr OS build v3.7.0 ***`, `*** Booting nRF Connect SDK v2.7.0 ***`,
    // `*** Booting MCUboot v2.1.0 ***`.
    r"\*\*\* Booting \S",
];

const BUILTIN_CRASH: &[&str] = &[
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
];

/// Longest custom rule accepted (characters).
const MAX_RULE_LEN: usize = 1024;

fn builtin_sets() -> &'static (RegexSet, RegexSet) {
    static SETS: OnceLock<(RegexSet, RegexSet)> = OnceLock::new();
    SETS.get_or_init(|| {
        (
            RegexSet::new(BUILTIN_BOOT).expect("valid boot regexes"),
            RegexSet::new(BUILTIN_CRASH).expect("valid crash regexes"),
        )
    })
}

fn compile_rules(rules: &[String], ctx: &str) -> Result<Option<RegexSet>, String> {
    if rules.is_empty() {
        return Ok(None);
    }
    for (i, rule) in rules.iter().enumerate() {
        if rule.is_empty() {
            return Err(format!("{ctx}[{i}] must not be empty"));
        }
        if rule.chars().count() > MAX_RULE_LEN {
            return Err(format!("{ctx}[{i}] exceeds {MAX_RULE_LEN} characters"));
        }
        Regex::new(rule).map_err(|e| format!("{ctx}[{i}] is not a valid regex: {e}"))?;
    }
    RegexSet::new(rules)
        .map(Some)
        .map_err(|e| format!("{ctx}: {e}"))
}

/// Stateful per-source detector (grouping looks at earlier lines).
#[derive(Debug)]
pub struct EventDetector {
    builtin: bool,
    boot: Option<RegexSet>,
    crash: Option<RegexSet>,
    window: Duration,
    last_boot: Option<DateTime<Local>>,
    last_crash: Option<DateTime<Local>>,
}

impl Default for EventDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl EventDetector {
    /// The built-in Zephyr rules.
    pub fn new() -> Self {
        Self {
            builtin: true,
            boot: None,
            crash: None,
            window: Duration::milliseconds(GROUP_WINDOW_MS),
            last_boot: None,
            last_crash: None,
        }
    }

    /// Detector for `sources.<ID>.events`; `Ok(None)` when detection is off.
    /// `ctx` is the YAML location used in error messages.
    pub fn for_source(events: Option<&SourceEvents>, ctx: &str) -> Result<Option<Self>, String> {
        match events {
            None => Ok(Some(Self::new())),
            Some(SourceEvents::Named(name)) if name == "zephyr" => Ok(Some(Self::new())),
            Some(SourceEvents::Named(name)) if name == "off" => Ok(None),
            Some(SourceEvents::Named(name)) => Err(format!(
                "{ctx}: unknown value '{name}' (use zephyr, off or rules with boot/crash regexes)"
            )),
            Some(SourceEvents::Rules(rules)) => {
                let window = rules.group_window_ms.unwrap_or(GROUP_WINDOW_MS as u64);
                if window > 600_000 {
                    return Err(format!("{ctx}.group_window_ms must be at most 600000"));
                }
                if !rules.builtin && rules.boot.is_empty() && rules.crash.is_empty() {
                    return Err(format!(
                        "{ctx}: builtin is false and no boot or crash rules are given (use `events: off` to disable detection)"
                    ));
                }
                Ok(Some(Self {
                    builtin: rules.builtin,
                    boot: compile_rules(&rules.boot, &format!("{ctx}.boot"))?,
                    crash: compile_rules(&rules.crash, &format!("{ctx}.crash"))?,
                    window: Duration::milliseconds(window as i64),
                    last_boot: None,
                    last_crash: None,
                }))
            }
        }
    }

    fn is_boot(&self, text: &str) -> bool {
        (self.builtin && builtin_sets().0.is_match(text))
            || self.boot.as_ref().is_some_and(|set| set.is_match(text))
    }

    fn is_crash(&self, text: &str) -> bool {
        (self.builtin && builtin_sets().1.is_match(text))
            || self.crash.as_ref().is_some_and(|set| set.is_match(text))
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
        let window = self.window;
        let recent = |last: Option<DateTime<Local>>| last.is_some_and(|last| at - last < window);
        if self.is_boot(text) {
            let new_boot = !recent(self.last_boot);
            self.last_boot = Some(at);
            self.last_crash = None;
            return new_boot.then_some(BOOT);
        }
        if self.is_crash(text) {
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
    fn custom_rules_extend_or_replace_the_builtin_ones() {
        let rules = |builtin: bool| {
            SourceEvents::Rules(crate::config::EventRules {
                builtin,
                boot: vec![r"^APP START v\d+".into()],
                crash: vec![r"Guru Meditation Error".into(), r"PANIC:".into()],
                group_window_ms: Some(100),
            })
        };
        let mut d = EventDetector::for_source(Some(&rules(true)), "ctx")
            .unwrap()
            .unwrap();
        assert_eq!(d.detect("APP START v2", at(0)), Some(BOOT));
        assert_eq!(
            d.detect("Guru Meditation Error: Core 0 panic'ed", at(10)),
            Some(CRASH)
        );
        assert_eq!(
            d.detect("PANIC: watchdog", at(50)),
            None,
            "same crash group"
        );
        assert_eq!(
            d.detect("PANIC: watchdog", at(500)),
            Some(CRASH),
            "100 ms window"
        );
        assert_eq!(
            d.detect("*** Booting Zephyr OS ***", at(900)),
            Some(BOOT),
            "builtin kept"
        );

        let mut d = EventDetector::for_source(Some(&rules(false)), "ctx")
            .unwrap()
            .unwrap();
        assert_eq!(
            d.detect("*** Booting Zephyr OS ***", at(0)),
            None,
            "builtin off"
        );
        assert_eq!(d.detect("APP START v3", at(0)), Some(BOOT));

        let off = SourceEvents::Named("off".into());
        assert!(EventDetector::for_source(Some(&off), "ctx")
            .unwrap()
            .is_none());
    }

    #[test]
    fn invalid_rules_report_their_yaml_location() {
        let bad = SourceEvents::Rules(crate::config::EventRules {
            builtin: true,
            boot: vec![],
            crash: vec!["ok".into(), "(unclosed".into()],
            group_window_ms: None,
        });
        let err = EventDetector::for_source(Some(&bad), "sources.APP.events").unwrap_err();
        assert!(
            err.starts_with("sources.APP.events.crash[1] is not a valid regex"),
            "{err}"
        );
        let unknown = SourceEvents::Named("zephir".into());
        assert!(
            EventDetector::for_source(Some(&unknown), "sources.APP.events")
                .unwrap_err()
                .contains("unknown value 'zephir'")
        );
        let empty = SourceEvents::Rules(crate::config::EventRules {
            builtin: false,
            boot: vec![],
            crash: vec![],
            group_window_ms: None,
        });
        assert!(EventDetector::for_source(Some(&empty), "x")
            .unwrap_err()
            .contains("events: off"));
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
