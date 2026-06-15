use std::collections::HashMap;
use std::path::Path;

use serde_json::json;
use tracing::{info, warn};

/// Load command suggestions from companion YAML files.
///
/// Resolution order:
/// 1. `<config-stem>.commands.yml` — alongside the main config file.
/// 2. `embed-log.commands.yml` in the config file directory.
/// 3. `embed-log.commands.yml` in the current working directory (only if
///    different from the config directory).
///
/// The first file that exists and contains valid commands is used.
///
/// Expected YAML shape:
/// ```yaml
/// sources:
///   DUT_UART:
///     - "help\r\n"
///     - "version\r\n"
/// ```
///
/// Only commands for the given `configured_sources` are kept, and only those
/// that correspond to writable sources (e.g., UART).
pub fn load_command_suggestions(
    config_path: Option<&Path>,
    configured_sources: &HashMap<String, bool>, // name → writable
) -> serde_json::Value {
    let config_dir = config_path
        .and_then(|p| p.parent())
        .unwrap_or(Path::new("."));
    let cwd = Path::new(".");

    // Collect candidates, deduplicating when config_dir == cwd.
    let mut candidates = Vec::new();

    // 1. <config-stem>.commands.yml — alongside the main config file.
    if let Some(p) = config_path {
        let stem = p.file_stem().unwrap_or_default();
        let parent = p.parent().unwrap_or(Path::new("."));
        candidates.push(parent.join(format!("{}.commands.yml", stem.to_string_lossy())));
    }

    // 2. embed-log.commands.yml in the config directory.
    let config_dir_fallback = config_dir.join("embed-log.commands.yml");
    candidates.push(config_dir_fallback);

    // 3. embed-log.commands.yml in CWD (if different from config dir).
    let cwd_fallback = cwd.join("embed-log.commands.yml");
    if cwd_fallback != config_dir.join("embed-log.commands.yml") {
        candidates.push(cwd_fallback);
    }

    for candidate in &candidates {
        if !candidate.exists() {
            continue;
        }
        match parse_command_file(candidate, configured_sources) {
            Ok(Some(commands)) => {
                info!("loaded command suggestions from {}", candidate.display());
                return commands;
            }
            Ok(None) => {
                info!(
                    "command file {} had no applicable commands",
                    candidate.display()
                );
                continue;
            }
            Err(e) => {
                warn!("failed to parse command file {}: {e}", candidate.display());
                continue;
            }
        }
    }

    json!({})
}

/// Parse a single command file and extract commands for configured sources.
///
/// Returns `Ok(None)` when the file is empty or has no applicable commands.
/// Returns `Err` when the file is malformed.
fn parse_command_file(
    path: &Path,
    configured_sources: &HashMap<String, bool>,
) -> Result<Option<serde_json::Value>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;

    if text.trim().is_empty() {
        return Ok(None);
    }

    let raw: serde_yaml::Value =
        serde_yaml::from_str(&text).map_err(|e| format!("invalid YAML: {e}"))?;

    // Expected shape: { "sources": { "NAME": ["cmd1", "cmd2", ...] } }
    let sources = raw
        .get("sources")
        .and_then(|v| v.as_mapping())
        .ok_or_else(|| format!("top-level 'sources' must be a mapping (got {:?})", raw))?;

    let mut result = serde_json::Map::new();

    for (key, commands_val) in sources {
        let source_name = match key.as_str() {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Skip unknown sources
        if !configured_sources.contains_key(&source_name) {
            warn!(
                "command file references unknown source '{}', ignoring",
                source_name
            );
            continue;
        }

        // Only expose commands for writable sources (UART)
        if !configured_sources
            .get(&source_name)
            .copied()
            .unwrap_or(false)
        {
            continue;
        }

        // Commands must be a list of strings
        let commands_list: Vec<String> = match commands_val {
            serde_yaml::Value::Sequence(seq) => seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .filter(|s| !s.is_empty())
                .collect(),
            _ => {
                warn!(
                    "commands for source '{}' must be a list, got {:?}",
                    source_name, commands_val
                );
                continue;
            }
        };

        if !commands_list.is_empty() {
            let json_arr: Vec<serde_json::Value> = commands_list
                .into_iter()
                .map(serde_json::Value::String)
                .collect();
            result.insert(source_name, serde_json::Value::Array(json_arr));
        }
    }

    if result.is_empty() {
        return Ok(None);
    }

    Ok(Some(serde_json::Value::Object(result)))
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
            "embed-log-commands-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn configured_sources_uart_only() -> HashMap<String, bool> {
        let mut map = HashMap::new();
        map.insert("DUT_UART".to_string(), true);
        map.insert("PYTEST".to_string(), false);
        map
    }

    /// Write a YAML commands file. The `cmds` values are raw strings containing
    /// actual newline/CRLF bytes. We write escaped versions so YAML's double-quoted
    /// string parsing produces the same bytes.
    fn write_yml(path: &Path, source: &str, cmds: &[&str]) {
        use std::io::Write;
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(f, "sources:").unwrap();
        writeln!(f, "  {}:", source).unwrap();
        for cmd in cmds {
            // Escape \r, \n, \t, \\, \" for YAML double-quoted strings
            let escaped: String = cmd
                .chars()
                .flat_map(|c| match c {
                    '\n' => "\\n".chars().collect(),
                    '\r' => "\\r".chars().collect(),
                    '\t' => "\\t".chars().collect(),
                    '\\' => "\\\\".chars().collect(),
                    '"' => "\\\"".chars().collect(),
                    _ => vec![c],
                })
                .collect();
            writeln!(f, "    - \"{}\"", escaped).unwrap();
        }
    }

    #[test]
    fn config_specific_commands_preferred_over_fallback() {
        let dir = temp_dir("preferred");
        let config_path = dir.join("test_config.yml");
        let specific = dir.join("test_config.commands.yml");
        let fallback = dir.join("embed-log.commands.yml");

        write_yml(&fallback, "DUT_UART", &["fallback_cmd\n"]);
        write_yml(&specific, "DUT_UART", &["specific_cmd\n"]);

        let result = load_command_suggestions(Some(&config_path), &configured_sources_uart_only());
        let commands = result["DUT_UART"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();
        assert_eq!(commands, vec!["specific_cmd\n"]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn fallback_loaded_when_config_specific_absent() {
        let dir = temp_dir("fallback");
        let config_path = dir.join("test_config.yml");
        let fallback = dir.join("embed-log.commands.yml");

        write_yml(&fallback, "DUT_UART", &["fallback_cmd\n"]);

        let result = load_command_suggestions(Some(&config_path), &configured_sources_uart_only());
        let commands = result["DUT_UART"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();
        assert_eq!(commands, vec!["fallback_cmd\n"]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unknown_source_names_ignored() {
        let dir = temp_dir("unknown");
        let config_path = dir.join("config.yml");
        let cmd_path = dir.join("config.commands.yml");

        write_yml(&cmd_path, "DUT_UART", &["ok\n"]);
        // Append extra unknown source
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&cmd_path)
            .unwrap();
        writeln!(f, "  NONEXISTENT:").unwrap();
        writeln!(f, "    - \"bad\\n\"").unwrap();

        let result = load_command_suggestions(Some(&config_path), &configured_sources_uart_only());
        assert!(result.get("NONEXISTENT").is_none());
        assert!(result.get("DUT_UART").is_some());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn non_uart_non_writable_sources_excluded() {
        let dir = temp_dir("nonwritable");
        let config_path = dir.join("config.yml");
        let cmd_path = dir.join("config.commands.yml");

        use std::io::Write;
        let mut f = std::fs::File::create(&cmd_path).unwrap();
        writeln!(f, "sources:").unwrap();
        writeln!(f, "  DUT_UART:").unwrap();
        writeln!(f, "    - \"uart_cmd\\n\"").unwrap();
        writeln!(f, "  PYTEST:").unwrap();
        writeln!(f, "    - \"udp_cmd\\n\"").unwrap();

        let result = load_command_suggestions(Some(&config_path), &configured_sources_uart_only());
        assert!(
            result.get("PYTEST").is_none(),
            "non-writable source should be excluded"
        );
        assert!(
            result.get("DUT_UART").is_some(),
            "writable source should be included"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn malformed_command_file_does_not_crash() {
        let dir = temp_dir("malformed");
        let config_path = dir.join("config.yml");
        let bad_path = dir.join("config.commands.yml");
        std::fs::write(&bad_path, "{{invalid yaml: [").unwrap();

        let result = load_command_suggestions(Some(&config_path), &configured_sources_uart_only());
        assert_eq!(result, json!({}));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn commands_included_in_config_ws_message() {
        let dir = temp_dir("wsmsg");
        let config_path = dir.join("config.yml");
        let cmd_path = dir.join("config.commands.yml");

        use std::io::Write;
        let mut f = std::fs::File::create(&cmd_path).unwrap();
        writeln!(f, "sources:").unwrap();
        writeln!(f, "  DUT_UART:").unwrap();
        writeln!(f, "    - \"help\\r\\n\"").unwrap();
        writeln!(f, "    - \"version\\r\\n\"").unwrap();

        let result = load_command_suggestions(Some(&config_path), &configured_sources_uart_only());
        let arr = result["DUT_UART"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_str().unwrap(), "help\r\n");
        assert_eq!(arr[1].as_str().unwrap(), "version\r\n");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn empty_command_file_returns_empty() {
        let dir = temp_dir("empty");
        let config_path = dir.join("config.yml");
        let empty_path = dir.join("config.commands.yml");
        std::fs::write(&empty_path, "").unwrap();

        let result = load_command_suggestions(Some(&config_path), &configured_sources_uart_only());
        assert_eq!(result, json!({}));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn no_config_path_uses_fallback_in_cwd() {
        let result = load_command_suggestions(None, &configured_sources_uart_only());
        assert!(result.is_object());
    }

    #[test]
    fn rotated_session_keeps_same_pane_commands() {
        let dir = temp_dir("rotate");
        let config_path = dir.join("config.yml");
        let cmd_path = dir.join("config.commands.yml");

        write_yml(&cmd_path, "DUT_UART", &["rotate_cmd\n"]);

        let pane_commands =
            load_command_suggestions(Some(&config_path), &configured_sources_uart_only());

        let rotated = pane_commands.clone();
        assert_eq!(rotated, pane_commands);
        assert_eq!(rotated["DUT_UART"][0].as_str().unwrap(), "rotate_cmd\n");

        std::fs::remove_dir_all(dir).ok();
    }
}
