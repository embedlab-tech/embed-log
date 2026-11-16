//! Log level detection per source.
//!
//! Each source has a level profile (the built-in `zephyr` one unless the YAML
//! says otherwise). The writer task runs one [`LevelDetector`] per source and
//! stores the detected level id in the record's `level` field, so the browser,
//! offline reports and API clients all see the same classification.

use std::collections::{BTreeMap, HashMap, HashSet};

use regex::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};

use crate::config::models::{
    AppConfig, LevelDefinition, LevelProfile, SourceConfig, SourceLevels, UnmatchedLevel,
};

/// Name of the built-in profile used when a source has no `levels` entry.
pub const BUILTIN_PROFILE: &str = "zephyr";
/// `levels: off` disables level detection and the level filter for a source.
pub const OFF: &str = "off";

/// Upper bound for a single level regex, mirroring the dictionary anchor limit.
const MAX_REGEX_LEN: usize = 1024;

fn level(id: &str, label: &str, regex: &str, default_visible: bool) -> LevelDefinition {
    LevelDefinition {
        id: id.into(),
        label: Some(label.into()),
        match_regex: Some(regex.into()),
        color: None,
        default_visible,
    }
}

/// Zephyr logging format: `[00:00:01.234,567] <inf> module: message`, the
/// dictionary decoder output (`<inf> module: ...`) and common `[ERROR]`-style
/// bracket tags. Dictionary hexdump rows (`    01 02 03 ...  |ascii`) belong
/// to the record above them.
pub fn builtin_zephyr_profile() -> LevelProfile {
    // Only a leading header counts; tags quoted in the message are data.
    // The header may follow stray control bytes (NUL, CR) and a shell prompt
    // that Zephyr erased with cursor escapes (`uart:~$ \x1b[8D\x1b[J…`, ANSI
    // is stripped before matching). Timestamps cover every log_output.c
    // format: ticks, `[00:00:01.234,567]`, Linux `[    1.234567]`, date
    // `[1970-01-01 00:00:01.234,567]`, ISO 8601 and the decoder's `[…Z]:`.
    let header = r"^[\x00-\x1f]*(?:[^\s\[<]*[>$#] )?(?:\[[0-9 ][0-9 T:.,+Z-]*\]:?[ \t]*)?";
    LevelProfile {
        levels: vec![
            level(
                "err",
                "Error",
                &format!(r"{header}(?:<err>|\[(?:err|error)\])"),
                true,
            ),
            level(
                "wrn",
                "Warning",
                &format!(r"{header}(?:<wrn>|\[(?:wrn|warn|warning)\])"),
                true,
            ),
            level(
                "inf",
                "Info",
                &format!(r"{header}(?:<inf>|\[(?:inf|info)\])"),
                true,
            ),
            level(
                "dbg",
                "Debug",
                &format!(r"{header}(?:<dbg>|\[(?:dbg|debug)\])"),
                false,
            ),
        ],
        case_insensitive: true,
        pattern: None,
        aliases: BTreeMap::new(),
        unmatched: UnmatchedLevel::None,
        continuation: Some(r"^ {4}(?:[0-9a-f]{2} )+ *\|".into()),
    }
}

/// The effective profile of a source, or `None` when detection is off.
/// Call only on a validated config (unknown names resolve to `None`).
pub fn resolve_source_profile(config: &AppConfig, source: &SourceConfig) -> Option<LevelProfile> {
    match &source.levels {
        None => Some(builtin_zephyr_profile()),
        Some(SourceLevels::Inline(profile)) => Some(profile.clone()),
        Some(SourceLevels::Named(name)) if name == OFF => None,
        Some(SourceLevels::Named(name)) => config
            .level_profiles
            .get(name)
            .cloned()
            .or_else(|| (name == BUILTIN_PROFILE).then(builtin_zephyr_profile)),
    }
}

/// UI-facing description of a profile: ids, labels, colors and defaults only.
/// Regexes stay on the server.
pub fn profile_summary(profile: &LevelProfile) -> serde_json::Value {
    serde_json::json!({
        "levels": profile.levels.iter().map(|level| serde_json::json!({
            "id": level.id,
            "label": level.label.clone().unwrap_or_else(|| level.id.clone()),
            "color": level.color,
            "default_visible": level.default_visible,
        })).collect::<Vec<_>>(),
    })
}

/// `{source: summary | null}` for the config message and offline reports.
pub fn source_levels_json(config: &AppConfig) -> serde_json::Value {
    serde_json::Value::Object(
        config
            .sources
            .iter()
            .map(|source| {
                let summary = resolve_source_profile(config, source)
                    .map(|profile| profile_summary(&profile))
                    .unwrap_or(serde_json::Value::Null);
                (source.name.clone(), summary)
            })
            .collect(),
    )
}

fn compile(regex: &str, case_insensitive: bool, ctx: &str) -> Result<Regex, String> {
    if regex.chars().count() > MAX_REGEX_LEN {
        return Err(format!("{ctx} exceeds {MAX_REGEX_LEN} characters"));
    }
    RegexBuilder::new(regex)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| format!("{ctx} is not a valid regex: {e}"))
}

fn valid_color(color: &str) -> bool {
    let hex = color.strip_prefix('#').is_some_and(|h| {
        matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
    });
    let name =
        !color.is_empty() && color.len() <= 32 && color.chars().all(|c| c.is_ascii_alphabetic());
    let variable = color
        .strip_prefix("var(--")
        .and_then(|v| v.strip_suffix(')'))
        .is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    hex || name || variable
}

/// Check one profile; `ctx` is the YAML location used in error messages.
pub fn validate_profile(profile: &LevelProfile, ctx: &str) -> Result<(), String> {
    LevelDetector::new(profile, ctx).map(|_| ())
}

/// Validate `level_profiles` and every `sources.*.levels` reference.
pub fn validate_level_config(config: &AppConfig) -> Result<(), String> {
    for (name, profile) in &config.level_profiles {
        if name == OFF || name == BUILTIN_PROFILE {
            return Err(format!(
                "level_profiles.{name}: '{name}' is a reserved profile name"
            ));
        }
        validate_profile(profile, &format!("level_profiles.{name}"))?;
    }
    for source in &config.sources {
        let ctx = format!("sources.{}.levels", source.name);
        match &source.levels {
            None => {}
            Some(SourceLevels::Inline(profile)) => validate_profile(profile, &ctx)?,
            Some(SourceLevels::Named(name))
                if name != OFF
                    && name != BUILTIN_PROFILE
                    && !config.level_profiles.contains_key(name) =>
            {
                let mut known: Vec<&str> = vec![BUILTIN_PROFILE, OFF];
                known.extend(config.level_profiles.keys().map(String::as_str));
                return Err(format!(
                    "{ctx}: unknown level profile '{name}' (known: {})",
                    known.join(", ")
                ));
            }
            Some(SourceLevels::Named(_)) => {}
        }
    }
    Ok(())
}

enum Matcher {
    /// One regex per level; the lowest matching index wins.
    PerLevel(RegexSet),
    /// One regex with a named `level` group mapped (case-insensitively) to an index.
    Pattern {
        regex: Regex,
        ids: HashMap<String, usize>,
    },
}

/// Stateful per-source classifier (`inherit` and `continuation` look at the
/// previous line). Not shared between sources.
pub struct LevelDetector {
    ids: Vec<String>,
    matcher: Matcher,
    continuation: Option<Regex>,
    inherit: bool,
    last: Option<usize>,
}

impl LevelDetector {
    /// Build and validate a detector. `ctx` prefixes error messages.
    pub fn new(profile: &LevelProfile, ctx: &str) -> Result<Self, String> {
        if profile.levels.is_empty() {
            return Err(format!("{ctx}.levels must list at least one level"));
        }
        let mut seen = HashSet::new();
        for (i, level) in profile.levels.iter().enumerate() {
            let lctx = format!("{ctx}.levels[{i}]");
            let id_ok = !level.id.is_empty()
                && level.id.len() <= 32
                && level
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if !id_ok {
                return Err(format!(
                    "{lctx}.id must be 1-32 characters of letters, digits, '_' or '-' (got {:?})",
                    level.id
                ));
            }
            if !seen.insert(level.id.to_ascii_lowercase()) {
                return Err(format!("{lctx}.id {:?} is defined twice", level.id));
            }
            if let Some(color) = &level.color {
                if !valid_color(color) {
                    return Err(format!(
                        "{lctx}.color must be #rgb, #rrggbb, a color name or var(--name) (got {color:?})"
                    ));
                }
            }
            if profile.pattern.is_none() && level.match_regex.as_deref().map_or(true, str::is_empty)
            {
                return Err(format!(
                    "{lctx}.match is required unless the profile defines `pattern`"
                ));
            }
        }
        let ids: Vec<String> = profile.levels.iter().map(|l| l.id.clone()).collect();
        let matcher = match &profile.pattern {
            Some(pattern) => {
                let regex = compile(pattern, profile.case_insensitive, &format!("{ctx}.pattern"))?;
                if !regex.capture_names().flatten().any(|name| name == "level") {
                    return Err(format!(
                        "{ctx}.pattern must contain a named group (?P<level>...)"
                    ));
                }
                let mut map: HashMap<String, usize> = ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| (id.to_lowercase(), i))
                    .collect();
                for (alias, target) in &profile.aliases {
                    let index = ids
                        .iter()
                        .position(|id| id.eq_ignore_ascii_case(target))
                        .ok_or_else(|| {
                            format!("{ctx}.aliases.{alias}: unknown level id {target:?}")
                        })?;
                    map.insert(alias.to_lowercase(), index);
                }
                Matcher::Pattern { regex, ids: map }
            }
            None => {
                if !profile.aliases.is_empty() {
                    return Err(format!("{ctx}.aliases requires `pattern`"));
                }
                let patterns: Vec<&str> = profile
                    .levels
                    .iter()
                    .map(|l| l.match_regex.as_deref().unwrap_or_default())
                    .collect();
                for (i, pattern) in patterns.iter().enumerate() {
                    // Compile individually for a precise error location.
                    compile(
                        pattern,
                        profile.case_insensitive,
                        &format!("{ctx}.levels[{i}].match"),
                    )?;
                }
                let set = RegexSetBuilder::new(&patterns)
                    .case_insensitive(profile.case_insensitive)
                    .build()
                    .map_err(|e| format!("{ctx}.levels: {e}"))?;
                Matcher::PerLevel(set)
            }
        };
        let continuation = profile
            .continuation
            .as_deref()
            .map(|c| compile(c, profile.case_insensitive, &format!("{ctx}.continuation")))
            .transpose()?;
        Ok(Self {
            ids,
            matcher,
            continuation,
            inherit: profile.unmatched == UnmatchedLevel::Inherit,
            last: None,
        })
    }

    /// Classify one received line (TX echoes should not be passed in).
    pub fn detect(&mut self, message: &str) -> Option<&str> {
        let stripped;
        let text = if message.contains('\x1b') {
            stripped = crate::postprocess::strip_ansi(message);
            stripped.as_str()
        } else {
            message
        };
        if self.continuation.as_ref().is_some_and(|c| c.is_match(text)) {
            return self.last.map(|i| self.ids[i].as_str());
        }
        let found = match &self.matcher {
            Matcher::PerLevel(set) => set.matches(text).iter().next(),
            Matcher::Pattern { regex, ids } => regex
                .captures(text)
                .and_then(|caps| caps.name("level"))
                .and_then(|m| ids.get(&m.as_str().to_lowercase()).copied()),
        };
        match found {
            Some(index) => self.last = Some(index),
            None if self.inherit => {}
            None => self.last = None,
        }
        if found.is_some() || self.inherit {
            self.last.map(|i| self.ids[i].as_str())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_all(profile: &LevelProfile, lines: &[&str]) -> Vec<Option<String>> {
        let mut detector = LevelDetector::new(profile, "test").unwrap();
        lines
            .iter()
            .map(|line| detector.detect(line).map(str::to_owned))
            .collect()
    }

    #[test]
    fn zephyr_profile_detects_text_dictionary_and_bracket_formats() {
        let got = detect_all(
            &builtin_zephyr_profile(),
            &[
                "[00:00:01.234,567] <inf> main: boot",
                "\x1b[1;31m[00:00:02.000,000] <err> spi: timeout\x1b[0m",
                "[00000042] <wrn> app: low battery",
                "<dbg> net: rx 12 bytes",
                "    01 02 03 04                                     |....",
                "uart:~$ help",
                "    01 02                                           |..",
                "[ERROR] legacy tag",
                "[Warning] mixed case",
                "plain printk output",
            ],
        );
        let expected = [
            Some("inf"),
            Some("err"),
            Some("wrn"),
            Some("dbg"),
            Some("dbg"), // hexdump continues the debug record
            None,        // shell output has no level
            None,        // and breaks the continuation chain
            Some("err"),
            Some("wrn"),
            None,
        ];
        assert_eq!(
            got,
            expected
                .iter()
                .map(|e| e.map(str::to_owned))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn zephyr_tags_in_message_bodies_do_not_override_the_header() {
        let mut detector = LevelDetector::new(&builtin_zephyr_profile(), "test").unwrap();
        for (text, expected) in [
            ("<dbg> app: received [ERROR]", Some("dbg")),
            ("[00000012] <dbg> app: received [ERROR]", Some("dbg")),
            ("[0000000000000012] <wrn> app: received <err>", Some("wrn")),
            (
                "[00:00:01.234,567] <inf> app: received [ERROR]",
                Some("inf"),
            ),
            (
                "[2024-01-01T00:00:00Z]: <dbg> app: received [ERROR]",
                Some("dbg"),
            ),
            ("[DEBUG] app: received [ERROR]", Some("dbg")),
            ("[warning] app: received <err>", Some("wrn")),
            ("help: use <dbg> to enable diagnostics", None),
            ("response contains [ERROR]", None),
            ("[00000012] help: use <dbg>", None),
            ("[not-a-timestamp] <dbg> app: text", None),
            ("<dbg] mismatched delimiters", None),
            ("[DEBUG> mismatched delimiters", None),
            // Headers after an erased shell prompt, control bytes and every
            // Zephyr timestamp format.
            (
                "device> \u{1b}[8D\u{1b}[J[00000000] <inf> rtc: interrupt configured",
                Some("inf"),
            ),
            (
                "uart:~$ \u{1b}[8D\u{1b}[J[00:00:01.000,000] <wrn> net: slow",
                Some("wrn"),
            ),
            ("\r[00:00:01.000,000] <err> main: cr", Some("err")),
            ("\0[0000000001] <err> main: nul", Some("err")),
            ("[    1.234567] <err> main: linux", Some("err")),
            (
                "[1970-01-01 00:00:01.234,567] <err> main: date",
                Some("err"),
            ),
            ("[1970-01-01T00:00:01,234567Z] <err> main: iso", Some("err")),
            ("a> b <dbg> x", None),
        ] {
            assert_eq!(detector.detect(text), expected, "{text}");
        }
    }

    #[test]
    fn indented_shell_output_is_not_mistaken_for_a_hexdump() {
        let got = detect_all(
            &builtin_zephyr_profile(),
            &[
                "<dbg> app: x",
                "  help  :Prints the help message",
                "    kernel  :Kernel commands",
            ],
        );
        assert_eq!(got, vec![Some("dbg".into()), None, None]);
    }

    #[test]
    fn first_matching_level_wins_and_inherit_keeps_previous_level() {
        let profile = LevelProfile {
            levels: vec![
                level("fatal", "Fatal", r"\bFATAL\b", true),
                level("err", "Error", r"\bE/", true),
                level("inf", "Info", r"\bI/", true),
            ],
            case_insensitive: false,
            pattern: None,
            aliases: BTreeMap::new(),
            unmatched: UnmatchedLevel::Inherit,
            continuation: None,
        };
        let got = detect_all(
            &profile,
            &[
                "boot",
                "E/net FATAL crash",
                "  at frame 1",
                "I/app ok",
                "continued",
            ],
        );
        assert_eq!(
            got,
            vec![
                None,
                Some("fatal".into()),
                Some("fatal".into()),
                Some("inf".into()),
                Some("inf".into())
            ]
        );
    }

    #[test]
    fn named_group_pattern_with_aliases() {
        let profile = LevelProfile {
            levels: vec![
                LevelDefinition {
                    id: "err".into(),
                    label: None,
                    match_regex: None,
                    color: Some("#ff0000".into()),
                    default_visible: true,
                },
                LevelDefinition {
                    id: "trace".into(),
                    label: Some("Trace".into()),
                    match_regex: None,
                    color: Some("var(--ansi-4)".into()),
                    default_visible: false,
                },
            ],
            case_insensitive: true,
            pattern: Some(r"^\[(?P<level>[A-Z]+)\]".into()),
            aliases: BTreeMap::from([("ERROR".into(), "err".into()), ("T".into(), "trace".into())]),
            unmatched: UnmatchedLevel::None,
            continuation: None,
        };
        let got = detect_all(
            &profile,
            &["[ERROR] x", "[err] y", "[T] z", "[INFO] w", "none"],
        );
        assert_eq!(
            got,
            vec![
                Some("err".into()),
                Some("err".into()),
                Some("trace".into()),
                None,
                None
            ]
        );
    }

    #[test]
    fn invalid_profiles_report_the_yaml_location() {
        let mut profile = builtin_zephyr_profile();
        profile.levels[1].match_regex = Some("(unclosed".into());
        let err = validate_profile(&profile, "level_profiles.fw").unwrap_err();
        assert!(err.contains("level_profiles.fw.levels[1].match"), "{err}");

        let mut profile = builtin_zephyr_profile();
        profile.levels[2].id = "err".into();
        assert!(validate_profile(&profile, "p")
            .unwrap_err()
            .contains("defined twice"));

        let mut profile = builtin_zephyr_profile();
        profile.levels[0].color = Some("red; background: url(x)".into());
        assert!(validate_profile(&profile, "p")
            .unwrap_err()
            .contains("color"));

        let mut profile = builtin_zephyr_profile();
        profile.pattern = Some(r"\[(\w+)\]".into());
        assert!(validate_profile(&profile, "p")
            .unwrap_err()
            .contains("(?P<level>"));

        let mut profile = builtin_zephyr_profile();
        profile.levels[0].match_regex = None;
        assert!(validate_profile(&profile, "p")
            .unwrap_err()
            .contains("match is required"));

        let profile = LevelProfile {
            levels: vec![],
            ..builtin_zephyr_profile()
        };
        assert!(validate_profile(&profile, "p")
            .unwrap_err()
            .contains("at least one level"));
    }

    #[test]
    fn summary_exposes_display_fields_but_not_regexes() {
        let summary = profile_summary(&builtin_zephyr_profile());
        let text = summary.to_string();
        assert!(!text.contains("<err>"), "{text}");
        assert_eq!(summary["levels"][3]["id"], "dbg");
        assert_eq!(summary["levels"][3]["default_visible"], false);
        assert_eq!(summary["levels"][0]["label"], "Error");
    }
}
