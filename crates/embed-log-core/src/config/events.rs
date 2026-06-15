use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use tracing::{info, warn};

use crate::config::models::EventRule;

/// Load event rules from companion YAML files.
///
/// Resolution order:
/// 1. `<config-stem>.events.yml` — alongside the main config file.
/// 2. `embed-log.events.yml` in the config file directory.
/// 3. `embed-log.events.yml` in the current working directory (only if
///    different from the config directory).
///
/// The first file that exists and contains valid rules is used.
///
/// Expected YAML shape:
/// ```yaml
/// DUT:
///   - name: fatal_error
///     pattern: "FATAL ERROR"
///     severity: error
///   - name: boot_complete
///     pattern: "boot complete"
///     severity: info
/// ```
///
/// Only rules for sources listed in `configured_sources` are kept.
/// Rules with invalid regex patterns are skipped with a warning.
/// Duplicate rule names within a source are rejected (returns error).
pub fn load_event_rules(
    config_path: Option<&Path>,
    configured_sources: &[String],
) -> Result<HashMap<String, Vec<EventRule>>, String> {
    let config_dir = config_path
        .and_then(|p| p.parent())
        .unwrap_or(Path::new("."));
    let cwd = Path::new(".");

    // Collect candidates, deduplicating when config_dir == cwd.
    let mut candidates = Vec::new();

    // 1. <config-stem>.events.yml — alongside the main config file.
    if let Some(p) = config_path {
        let stem = p.file_stem().unwrap_or_default();
        let parent = p.parent().unwrap_or(Path::new("."));
        candidates.push(parent.join(format!("{}.events.yml", stem.to_string_lossy())));
    }

    // 2. embed-log.events.yml in the config directory.
    let config_dir_fallback = config_dir.join("embed-log.events.yml");
    candidates.push(config_dir_fallback);

    // 3. embed-log.events.yml in CWD (if different from config dir).
    let cwd_fallback = cwd.join("embed-log.events.yml");
    if cwd_fallback != config_dir.join("embed-log.events.yml") {
        candidates.push(cwd_fallback);
    }

    for candidate in &candidates {
        if !candidate.exists() {
            continue;
        }
        match parse_event_file(candidate, configured_sources) {
            Ok(Some(rules)) => {
                info!("loaded event rules from {}", candidate.display());
                return Ok(rules);
            }
            Ok(None) => {
                info!("event file {} had no applicable rules", candidate.display());
                continue;
            }
            Err(e) => {
                warn!("failed to parse event file {}: {e}", candidate.display());
                return Err(e);
            }
        }
    }

    Ok(HashMap::new())
}

/// Parse a single event rules file and return rules keyed by source name.
///
/// Returns `Ok(None)` when the file is empty or has no applicable rules.
/// Returns `Err` when the file is malformed or has contradictory content.
fn parse_event_file(
    path: &Path,
    configured_sources: &[String],
) -> Result<Option<HashMap<String, Vec<EventRule>>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;

    if text.trim().is_empty() {
        return Ok(None);
    }

    let raw: serde_yaml::Value =
        serde_yaml::from_str(&text).map_err(|e| format!("invalid YAML: {e}"))?;

    // Top level is a mapping of source name → array of rule definitions.
    let sources = raw.as_mapping().ok_or_else(|| {
        format!(
            "event file must be a mapping at the top level, got {:?}",
            raw
        )
    })?;

    let mut result: HashMap<String, Vec<EventRule>> = HashMap::new();

    for (key, rules_val) in sources {
        let source_name = match key.as_str() {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Skip unknown sources
        if !configured_sources.contains(&source_name) {
            warn!(
                "event file references unknown source '{}', ignoring",
                source_name
            );
            continue;
        }

        // Rules must be a sequence
        let rules_seq = match rules_val.as_sequence() {
            Some(seq) => seq,
            None => {
                warn!(
                    "event rules for source '{}' must be a list, got {:?}",
                    source_name, rules_val
                );
                continue;
            }
        };

        let mut source_rules: Vec<EventRule> = Vec::new();

        for rule_val in rules_seq {
            let rule_map = match rule_val.as_mapping() {
                Some(m) => m,
                None => {
                    warn!(
                        "event rule for source '{}' is not a mapping, got {:?}",
                        source_name, rule_val
                    );
                    continue;
                }
            };

            let name = match rule_map.get("name") {
                Some(serde_yaml::Value::String(n)) => n.clone(),
                _ => {
                    warn!(
                        "event rule for source '{}' is missing a 'name' field",
                        source_name
                    );
                    continue;
                }
            };

            let pattern = match rule_map.get("pattern") {
                Some(serde_yaml::Value::String(p)) => p.clone(),
                _ => {
                    warn!(
                        "event rule '{}' for source '{}' is missing a 'pattern' field",
                        name, source_name
                    );
                    continue;
                }
            };

            let severity = match rule_map.get("severity") {
                Some(serde_yaml::Value::String(s)) => match s.as_str() {
                    "info" | "warn" | "error" | "fatal" => s.clone(),
                    _ => {
                        warn!(
                            "event rule '{}' for source '{}' has invalid severity '{}', defaulting to 'info'",
                            name, source_name, s
                        );
                        "info".to_string()
                    }
                },
                _ => "info".to_string(),
            };

            // Validate uniqueness within source
            if source_rules.iter().any(|r| r.name == name) {
                return Err(format!(
                    "duplicate event rule name '{}' for source '{}'",
                    name, source_name
                ));
            }

            // Compile regex
            let regex = match Regex::new(&pattern) {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        "invalid regex pattern '{}' in rule '{}' for source '{}': {e}",
                        pattern, name, source_name
                    );
                    continue; // skip invalid rules
                }
            };

            source_rules.push(EventRule {
                name,
                pattern,
                severity,
                regex,
            });
        }

        if !source_rules.is_empty() {
            result.insert(source_name, source_rules);
        }
    }

    if result.is_empty() {
        return Ok(None);
    }

    Ok(Some(result))
}

/// A match result from checking a line against a rule.
#[derive(Debug, Clone)]
pub struct EventMatch {
    /// Name of the matched rule.
    pub rule_name: String,
    /// Severity of the matched rule.
    pub severity: String,
    /// Regex capture groups.
    pub captures: Vec<String>,
}

/// A compiled pattern matcher for one source.
#[derive(Debug, Clone)]
pub struct PatternMatcher {
    rules: Vec<EventRule>,
}

impl PatternMatcher {
    /// Create a new matcher from a list of rules.
    pub fn new(rules: Vec<EventRule>) -> Self {
        Self { rules }
    }

    /// Check a message against all rules, returning all matches.
    pub fn check(&self, message: &str) -> Vec<EventMatch> {
        let mut results = Vec::new();
        for rule in &self.rules {
            if let Some(captures) = rule.regex.captures(message) {
                let captures_vec: Vec<String> = captures
                    .iter()
                    .filter_map(|m| m.map(|c| c.as_str().to_string()))
                    .collect();
                results.push(EventMatch {
                    rule_name: rule.name.clone(),
                    severity: rule.severity.clone(),
                    captures: captures_vec,
                });
            }
        }
        results
    }

    /// Return true if this matcher has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Access the underlying rules (for building config metadata).
    pub fn rules(&self) -> &[EventRule] {
        &self.rules
    }
}

/// Load event rules and build a matcher per source.
pub fn load_event_matchers(
    config_path: Option<&Path>,
    configured_sources: &[String],
) -> HashMap<String, PatternMatcher> {
    match load_event_rules(config_path, configured_sources) {
        Ok(rules_map) => rules_map
            .into_iter()
            .map(|(name, rules)| (name, PatternMatcher::new(rules)))
            .collect(),
        Err(e) => {
            warn!("failed to load event rules: {e}");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let dir = std::env::temp_dir().join(format!(
            "embed-log-events-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn configured_sources() -> Vec<String> {
        vec!["DUT".to_string(), "HOST".to_string()]
    }

    fn write_event_yml(path: &Path, yaml: &str) {
        std::fs::write(path, yaml).unwrap();
    }

    #[test]
    fn valid_file_parses_and_compiles_regexes() {
        let dir = temp_dir("valid");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        write_event_yml(
            &events_path,
            r#"
DUT:
  - name: fatal_error
    pattern: "FATAL ERROR"
    severity: error
  - name: boot_complete
    pattern: "boot complete"
    severity: info
HOST:
  - name: test_passed
    pattern: PASSED
    severity: info
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["DUT"].len(), 2);
        assert_eq!(result["HOST"].len(), 1);

        // Verify regexes compile and match
        assert!(result["DUT"][0].regex.is_match("FATAL ERROR"));
        assert!(result["DUT"][1].regex.is_match("boot complete"));
        assert!(result["HOST"][0].regex.is_match("PASSED"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn invalid_regex_skips_rule() {
        let dir = temp_dir("invalid_regex");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        write_event_yml(
            &events_path,
            r#"
DUT:
  - name: good
    pattern: "valid.*pattern"
    severity: info
  - name: bad_regex
    pattern: "[invalid"
    severity: error
  - name: another_good
    pattern: "ok"
    severity: info
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["DUT"].len(), 2);
        assert!(result["DUT"].iter().all(|r| r.name != "bad_regex"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn duplicate_rule_name_returns_error() {
        let dir = temp_dir("duplicate");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        // Use a helper that writes proper indentation
        use std::io::Write;
        let mut f = std::fs::File::create(&events_path).unwrap();
        writeln!(f, "DUT:").unwrap();
        writeln!(f, "  - name: same_name").unwrap();
        writeln!(f, "    pattern: first").unwrap();
        writeln!(f, "    severity: info").unwrap();
        writeln!(f, "  - name: same_name").unwrap();
        writeln!(f, "    pattern: second").unwrap();
        writeln!(f, "    severity: error").unwrap();

        let result = load_event_rules(Some(&config_path), &configured_sources());
        if let Err(ref e) = result {
            assert!(
                e.contains("duplicate"),
                "error should mention duplicate, got: {e}"
            );
        } else {
            panic!("expected Err, got Ok: {:?}", result.unwrap());
        }

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unknown_source_skipped_with_warning() {
        let dir = temp_dir("unknown_source");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        write_event_yml(
            &events_path,
            r#"
DUT:
  - name: ok
    pattern: "hello"
    severity: info
NONEXISTENT:
  - name: ghost
    pattern: "boo"
    severity: warn
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("DUT"));
        assert!(!result.contains_key("NONEXISTENT"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn config_specific_rules_preferred_over_fallback() {
        let dir = temp_dir("preferred");
        let config_path = dir.join("test_config.yml");
        let specific = dir.join("test_config.events.yml");
        let fallback = dir.join("embed-log.events.yml");

        write_event_yml(
            &fallback,
            r#"
DUT:
  - name: fallback_rule
    pattern: "fallback"
    severity: info
"#,
        );
        write_event_yml(
            &specific,
            r#"
DUT:
  - name: specific_rule
    pattern: "specific"
    severity: warn
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["DUT"][0].name, "specific_rule");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn fallback_loaded_when_config_specific_absent() {
        let dir = temp_dir("fallback");
        let config_path = dir.join("test_config.yml");
        let fallback = dir.join("embed-log.events.yml");

        write_event_yml(
            &fallback,
            r#"
DUT:
  - name: fallback_rule
    pattern: "fallback"
    severity: info
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["DUT"][0].name, "fallback_rule");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_file_returns_empty_map() {
        let dir = temp_dir("missing");
        let config_path = dir.join("config.yml");

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert!(result.is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn empty_file_returns_empty_map() {
        let dir = temp_dir("empty");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        std::fs::write(&events_path, "").unwrap();

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert!(result.is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn no_config_path_uses_fallback_in_cwd() {
        let result = load_event_rules(None, &configured_sources()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn rule_missing_name_field_skipped() {
        let dir = temp_dir("missing_name");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        write_event_yml(
            &events_path,
            r#"
DUT:
  - pattern: "no-name"
    severity: info
  - name: has_name
    pattern: "good"
    severity: info
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["DUT"].len(), 1);
        assert_eq!(result["DUT"][0].name, "has_name");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rule_missing_pattern_field_skipped() {
        let dir = temp_dir("missing_pattern");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        write_event_yml(
            &events_path,
            r#"
DUT:
  - name: no_pattern
    severity: error
  - name: good
    pattern: "yes"
    severity: info
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["DUT"].len(), 1);
        assert_eq!(result["DUT"][0].name, "good");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn severity_defaults_to_info() {
        let dir = temp_dir("severity_default");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        write_event_yml(
            &events_path,
            r#"
DUT:
  - name: no_severity
    pattern: "hello"
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(result["DUT"][0].severity, "info");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn invalid_severity_defaults_to_info() {
        let dir = temp_dir("invalid_severity");
        let config_path = dir.join("config.yml");
        let events_path = dir.join("config.events.yml");

        write_event_yml(
            &events_path,
            r#"
DUT:
  - name: bad_sev
    pattern: "test"
    severity: bogus
"#,
        );

        let result = load_event_rules(Some(&config_path), &configured_sources()).unwrap();
        assert_eq!(
            result["DUT"][0].severity, "info",
            "invalid severity 'bogus' should default to 'info'"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pattern_matcher_capture_groups() {
        // Use a regex with capture groups: (ERROR|WARN): (.+)
        let matcher = PatternMatcher::new(vec![EventRule {
            name: "severity_capture".to_string(),
            pattern: "(ERROR|WARN): (.+)".to_string(),
            severity: "error".to_string(),
            regex: regex::Regex::new("(ERROR|WARN): (.+)").unwrap(),
        }]);

        let matches = matcher.check("ERROR: something went wrong");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rule_name, "severity_capture");
        // Group 0 is the full match, group 1 is ERROR, group 2 is "something went wrong"
        assert_eq!(matches[0].captures[0], "ERROR: something went wrong");
        assert_eq!(matches[0].captures[1], "ERROR");
        assert_eq!(matches[0].captures[2], "something went wrong");

        // Also test WARN branch
        let matches2 = matcher.check("WARN: low battery");
        assert_eq!(matches2.len(), 1);
        assert_eq!(matches2[0].captures[1], "WARN");
        assert_eq!(matches2[0].captures[2], "low battery");

        // Non-matching line
        let matches3 = matcher.check("boot complete");
        assert!(matches3.is_empty());
    }
}
