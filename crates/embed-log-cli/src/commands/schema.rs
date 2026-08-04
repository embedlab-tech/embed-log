//! Machine-readable CLI capability discovery.
//!
//! `--help` remains the human interface. This module renders Clap's actual
//! command/argument graph and augments it with the semantics agents cannot
//! infer from flags alone: mutation, targeting, limits, outputs, and stable
//! error codes.

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};
use serde_json::{json, Map, Value};

pub(crate) const SCHEMA_VERSION: u32 = 1;

pub(crate) fn cmd_schema(root: Command, selector: &[String], pretty: bool) -> Result<()> {
    let value = schema_value(root, selector)?;
    if pretty {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{}", serde_json::to_string(&value)?);
    }
    Ok(())
}

pub(crate) fn schema_value(mut root: Command, selector: &[String]) -> Result<Value> {
    root.build();
    let normalized = normalize_selector(selector);
    match normalized.as_str() {
        "" => Ok(capability_index(&root)),
        "errors" => Ok(error_catalog()),
        "config" => Ok(config_capabilities()),
        path => {
            let command = find_command(&root, path).with_context(|| {
                format!(
                    "unknown schema selector {path:?}; run `embed-log schema` for the capability index"
                )
            })?;
            Ok(command_schema(path, command))
        }
    }
}

fn normalize_selector(selector: &[String]) -> String {
    if selector.len() == 1 {
        selector[0].trim().replace(' ', ".")
    } else {
        selector
            .iter()
            .map(|part| part.trim_matches('.'))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(".")
    }
}

fn capability_index(root: &Command) -> Value {
    let mut commands = Vec::new();
    collect_command_paths(root, "", &mut commands);
    json!({
        "schema_version": SCHEMA_VERSION,
        "embed_log_version": env!("CARGO_PKG_VERSION"),
        "kind": "embed-log.capabilities",
        "commands": commands,
        "topics": ["config", "errors"],
        "interfaces": ["browser", "tui", "http", "websocket", "cli"],
        "source_types": ["uart", "file", "udp"],
        "parser_types": ["text", "slip-coap", "zephyr-dict"],
        "config_version": 2,
        "defaults": {
            "endpoint": "127.0.0.1:18080",
            "read_records": 100,
            "time_display": "relative",
            "watch_ttl": "30s"
        },
        "limits": {
            "read_records_max": 1000,
            "around_records_max": 1000,
            "tx_context_records_max": 1000,
            "watch_ttl_max": "24h"
        },
        "discovery": {
            "command": "embed-log schema <command>",
            "examples": [
                "embed-log schema sessions.read",
                "embed-log schema tx",
                "embed-log schema errors",
                "embed-log schema config"
            ]
        }
    })
}

fn collect_command_paths(command: &Command, prefix: &str, output: &mut Vec<String>) {
    for subcommand in command
        .get_subcommands()
        .filter(|cmd| !cmd.is_hide_set() && cmd.get_name() != "help")
    {
        let path = if prefix.is_empty() {
            subcommand.get_name().to_string()
        } else {
            format!("{prefix}.{}", subcommand.get_name())
        };
        output.push(path.clone());
        collect_command_paths(subcommand, &path, output);
    }
}

fn find_command<'a>(root: &'a Command, path: &str) -> Option<&'a Command> {
    let mut current = root;
    for segment in path.split('.') {
        current = current.get_subcommands().find(|command| {
            command.get_name() == segment
                || command.get_visible_aliases().any(|alias| alias == segment)
        })?;
    }
    (!current.is_hide_set()).then_some(current)
}

fn command_schema(path: &str, command: &Command) -> Value {
    let arguments = command
        .get_arguments()
        .filter(|arg| !arg.is_hide_set() && arg.get_id() != "help" && arg.get_id() != "version")
        .map(|arg| argument_schema(command, arg))
        .collect::<Vec<_>>();
    let semantics = semantics(path);
    let subcommands = command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set() && subcommand.get_name() != "help")
        .map(|subcommand| subcommand.get_name().to_string())
        .collect::<Vec<_>>();
    let aliases = command
        .get_visible_aliases()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut rendered = command.clone();
    let usage = rendered.render_usage().to_string();

    json!({
        "schema_version": SCHEMA_VERSION,
        "embed_log_version": env!("CARGO_PKG_VERSION"),
        "kind": "embed-log.command",
        "command": path,
        "about": command.get_about().map(ToString::to_string),
        "usage": usage,
        "aliases": aliases,
        "subcommands": subcommands,
        "mutates": semantics.mutates,
        "execution": semantics.execution,
        "targeting": semantics.targeting,
        "arguments": arguments,
        "output": semantics.output,
        "errors": semantics.errors,
        "notes": semantics.notes
    })
}

fn argument_schema(command: &Command, arg: &Arg) -> Value {
    let action = action_name(arg.get_action());
    let possible_values = arg
        .get_value_parser()
        .possible_values()
        .map(|values| {
            values
                .map(|value| value.get_name().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let defaults = arg
        .get_default_values()
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let value_names = arg
        .get_value_names()
        .map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    let conflicts = command
        .get_arg_conflicts_with(arg)
        .into_iter()
        .filter(|other| !other.is_hide_set())
        .map(|other| other.get_id().to_string())
        .collect::<Vec<_>>();

    let mut value = Map::new();
    let id = arg.get_id().as_str();
    value.insert("id".into(), json!(id));
    if let Some(index) = arg.get_index() {
        value.insert("position".into(), json!(index));
    }
    if let Some(long) = arg.get_long() {
        value.insert("long".into(), json!(format!("--{long}")));
    }
    if let Some(short) = arg.get_short() {
        value.insert("short".into(), json!(format!("-{short}")));
    }
    let aliases = arg
        .get_visible_aliases()
        .map(|aliases| {
            aliases
                .iter()
                .map(|alias| format!("--{alias}"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !aliases.is_empty() {
        value.insert("aliases".into(), json!(aliases));
    }
    value.insert("required".into(), json!(arg.is_required_set()));
    value.insert("action".into(), json!(action));
    value.insert(
        "type".into(),
        json!(argument_type(command.get_name(), id, arg, &possible_values)),
    );
    if matches!(arg.get_action(), ArgAction::Append | ArgAction::Count) {
        value.insert("repeatable".into(), json!(true));
    }
    if !value_names.is_empty() {
        value.insert("value_names".into(), json!(value_names));
    }
    if !possible_values.is_empty() {
        value.insert("enum".into(), json!(possible_values));
    }
    if !defaults.is_empty() {
        value.insert("default".into(), json!(defaults));
    }
    if !conflicts.is_empty() {
        value.insert("conflicts_with".into(), json!(conflicts));
    }
    if let Some(help) = arg.get_help() {
        value.insert("description".into(), json!(help.to_string()));
    }
    add_known_constraints(command.get_name(), id, &mut value);
    Value::Object(value)
}

fn argument_type(
    command_name: &str,
    id: &str,
    arg: &Arg,
    possible_values: &[String],
) -> &'static str {
    if matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse) {
        return "boolean";
    }
    if !possible_values.is_empty() {
        return "enum";
    }
    if matches!(id, "timeout" | "ttl" | "since") {
        return "duration";
    }
    if matches!(
        id,
        "baud"
            | "ws_port"
            | "context"
            | "limit"
            | "last"
            | "lines"
            | "after"
            | "before"
            | "sequence"
            | "before_context"
            | "after_context"
    ) {
        return "integer";
    }
    if matches!(
        id,
        "config"
            | "frontend_dir"
            | "file"
            | "serial"
            | "serial_paths"
            | "save_config"
            | "log_dir"
            | "dir"
            | "output"
    ) {
        return "path";
    }
    if id == "url" {
        return "url";
    }
    if command_name == "run" && id == "host" {
        return "host";
    }
    "string"
}

fn add_known_constraints(command_name: &str, id: &str, value: &mut Map<String, Value>) {
    match (command_name, id) {
        ("read", "limit") => {
            value.insert("minimum".into(), json!(1));
            value.insert("maximum".into(), json!(1000));
        }
        ("read", "last") | ("tx", "context") => {
            value.insert("minimum".into(), json!(0));
            value.insert("maximum".into(), json!(1000));
        }
        ("around", "before" | "after") => {
            value.insert("minimum".into(), json!(0));
            value.insert(
                "group_constraint".into(),
                json!("before + after + target <= 1000"),
            );
        }
        ("around", "sequence") => {
            value.insert("minimum".into(), json!(1));
        }
        ("read", "after") => {
            value.insert("minimum".into(), json!(0));
            value.insert("exclusive_cursor".into(), json!(true));
        }
        ("add", "ttl") => {
            value.insert("maximum".into(), json!("24h"));
        }
        ("run", "ws_port") => {
            value.insert("minimum".into(), json!(1));
            value.insert("maximum".into(), json!(65535));
        }
        ("run", "baud") => {
            value.insert("minimum".into(), json!(1));
        }
        _ => {}
    }
}

fn action_name(action: &ArgAction) -> &'static str {
    match action {
        ArgAction::Set => "set",
        ArgAction::Append => "append",
        ArgAction::SetTrue => "set_true",
        ArgAction::SetFalse => "set_false",
        ArgAction::Count => "count",
        ArgAction::Help => "help",
        ArgAction::HelpShort => "help_short",
        ArgAction::HelpLong => "help_long",
        ArgAction::Version => "version",
        _ => "other",
    }
}

struct Semantics {
    mutates: bool,
    execution: &'static str,
    targeting: Value,
    output: Value,
    errors: &'static [&'static str],
    notes: &'static [&'static str],
}

fn semantics(path: &str) -> Semantics {
    let local = json!({"mode":"local","daemon_required":false});
    let read_target = json!({
        "mode":"daemon_or_url",
        "daemon_required":true,
        "instance_env":"EMBED_LOG_INSTANCE",
        "sole_instance_inference":true
    });
    let mutation_target = json!({
        "mode":"explicit_daemon_or_url",
        "daemon_required":true,
        "instance_env":"EMBED_LOG_INSTANCE",
        "sole_instance_inference":false
    });
    let session_target = json!({
        "mode":"offline_session",
        "resolution":["exact_id","unique_prefix","latest"]
    });
    let json_or_text = json!({"modes":["text","json"]});
    let compact_cursor = json!({
        "modes":["text","compact_json","full_json"],
        "default_time":"relative",
        "cursor":"sequence",
        "next_cursor":"next_cursor",
        "bounded":true
    });

    match path {
        "watch" | "sessions" => Semantics {
            mutates: false,
            execution: "command_group",
            targeting: Value::Null,
            output: json!({"modes":[]}),
            errors: &[],
            notes: &["select one advertised subcommand"],
        },
        "run" => Semantics {
            mutates: true,
            execution: "server",
            targeting: local,
            output: json!({"modes":["human","json_with_daemon"]}),
            errors: &[],
            notes: &["daemon mode requires explicit --config, --instance, and --port"],
        },
        "status" => Semantics {
            mutates: false,
            execution: "daemon",
            targeting: read_target,
            output: json_or_text,
            errors: &[],
            notes: &["read-only status may infer the sole registered daemon"],
        },
        "stop" => Semantics {
            mutates: true,
            execution: "daemon",
            targeting: json!({"mode":"explicit_registered_daemon","instance_env":"EMBED_LOG_INSTANCE","sole_instance_inference":false}),
            output: json_or_text,
            errors: &[],
            notes: &[],
        },
        "tx" => Semantics {
            mutates: true,
            execution: "daemon",
            targeting: mutation_target,
            output: json!({"modes":["text","json"],"bounded_context":true,"next_cursor_on_expectation":true}),
            errors: &["EXPECT_TIMEOUT"],
            notes: &[
                "exactly one of --line, --raw, --file, or --stdin is required",
                "expectations are armed before write and match RX records only",
                "--context is capped at 1000",
            ],
        },
        "watch.add" => Semantics {
            mutates: true,
            execution: "daemon",
            targeting: mutation_target,
            output: json_or_text,
            errors: &["WATCH_ERROR"],
            notes: &[
                "exactly one of --contains or --regex is required",
                "temporary watches are process-local and are not exported",
            ],
        },
        "watch.remove" => Semantics {
            mutates: true,
            execution: "daemon",
            targeting: mutation_target,
            output: json_or_text,
            errors: &["WATCH_NOT_FOUND", "WATCH_ERROR"],
            notes: &["temporary watches are process-local and are not exported"],
        },
        "watch.wait" => Semantics {
            mutates: false,
            execution: "daemon",
            targeting: mutation_target,
            output: json!({"modes":["text","json"],"next_cursor_on_match":true}),
            errors: &["WATCH_EXPIRED", "WATCH_WAIT_TIMEOUT", "WATCH_NOT_FOUND", "WATCH_ERROR"],
            notes: &["ordinary logs are not streamed while waiting"],
        },
        "sessions.new" => Semantics {
            mutates: true,
            execution: "daemon",
            targeting: mutation_target,
            output: json_or_text,
            errors: &[],
            notes: &["rotation retains source tasks and UART ownership"],
        },
        "sessions.read" => Semantics {
            mutates: false,
            execution: "offline",
            targeting: session_target,
            output: compact_cursor,
            errors: &[],
            notes: &["--after is exclusive and globally applied before source filtering", "default limit is 100; hard limit is 1000", "legacy sessions without global sequence are rejected"],
        },
        "sessions.around" => Semantics {
            mutates: false,
            execution: "offline",
            targeting: session_target,
            output: compact_cursor,
            errors: &[],
            notes: &["target plus before and after context is capped at 1000 records", "event IDs must identify exactly one persisted event"],
        },
        "sessions.list" | "sessions.search" => Semantics {
            mutates: false,
            execution: "offline",
            targeting: json!({"mode":"offline_logs_directory"}),
            output: json_or_text,
            errors: &[],
            notes: &[],
        },
        path if path.starts_with("sessions.") => Semantics {
            mutates: path == "sessions.export",
            execution: "offline",
            targeting: session_target,
            output: json_or_text,
            errors: &[],
            notes: &[],
        },
        "validate" | "version" | "doctor" | "ports" => Semantics {
            mutates: false,
            execution: "local",
            targeting: local,
            output: json_or_text,
            errors: &[],
            notes: &[],
        },
        "schema" => Semantics {
            mutates: false,
            execution: "local",
            targeting: local,
            output: json!({"modes":["compact_json","pretty_json"],"stdout_documents":1}),
            errors: &["SCHEMA_SELECTOR_NOT_FOUND"],
            notes: &["schema output contains no runtime state and can be cached by schema_version and embed_log_version"],
        },
        _ => Semantics {
            mutates: false,
            execution: "local",
            targeting: local,
            output: json_or_text,
            errors: &[],
            notes: &[],
        },
    }
}

fn error_catalog() -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "embed_log_version": env!("CARGO_PKG_VERSION"),
        "kind": "embed-log.errors",
        "coverage": "all_json_invocations",
        "contract": {"ok":false,"error":{"code":"<stable code>","message":"<human detail>","details":"<object or null>"}},
        "note": "Every invocation requesting JSON emits one JSON failure document on stdout and exits nonzero. COMMAND_FAILED is the stable fallback when no narrower code applies.",
        "errors": [
            {"code":"CLI_USAGE","commands":["*"],"meaning":"Clap rejected arguments for an invocation requesting JSON."},
            {"code":"COMMAND_FAILED","commands":["*"],"meaning":"The command failed without a narrower stable classification."},
            {"code":"CONFIG_NOT_FOUND","commands":["run","validate","doctor"],"meaning":"The selected configuration file does not exist."},
            {"code":"INSTANCE_REQUIRED","commands":["stop","tx","watch.*","sessions.new"],"meaning":"A mutating command had no explicit daemon target."},
            {"code":"INSTANCE_NOT_FOUND","commands":["status","stop","tx","watch.*","sessions.new"],"meaning":"The selected registered daemon instance does not exist."},
            {"code":"SESSION_NOT_FOUND","commands":["sessions.*"],"meaning":"The selected offline session could not be resolved."},
            {"code":"SOURCE_NOT_FOUND","commands":["tx","watch.add","sessions.read"],"meaning":"The selected source does not exist."},
            {"code":"SOURCE_NOT_WRITABLE","commands":["tx"],"meaning":"The selected source cannot accept TX."},
            {"code":"CURSOR_INVALID","commands":["sessions.read","sessions.around"],"meaning":"Stored sequence or requested cursor/context is invalid."},
            {"code":"SCHEMA_SELECTOR_NOT_FOUND","commands":["schema"],"meaning":"The requested command or schema topic is unknown."},
            {"code":"EXPECT_TIMEOUT","commands":["tx"],"meaning":"No matching RX record arrived before the expectation deadline."},
            {"code":"WATCH_EXPIRED","commands":["watch.wait"],"meaning":"The server-side watch TTL elapsed before a match."},
            {"code":"WATCH_WAIT_TIMEOUT","commands":["watch.wait"],"meaning":"This wait invocation ended while the server-side watch remains active."},
            {"code":"WATCH_NOT_FOUND","commands":["watch.wait","watch.remove"],"meaning":"The watch ID is unknown to the selected process."},
            {"code":"WATCH_ERROR","commands":["watch.add","watch.wait","watch.remove"],"meaning":"The watch control operation failed without a more specific stable code."}
        ]
    })
}

fn config_capabilities() -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "embed_log_version": env!("CARGO_PKG_VERSION"),
        "kind": "embed-log.config-capabilities",
        "canonical_version": 2,
        "format": "yaml",
        "strict_unknown_fields": true,
        "top_level": ["version", "server", "logs", "sources", "ui"],
        "source_types": ["uart", "file", "udp"],
        "parser_types": ["text", "slip-coap", "zephyr-dict"],
        "default_endpoint": "127.0.0.1:18080",
        "validation_command": "embed-log validate --config <PATH> --json",
        "documentation": "docs/configuration.md",
        "note": "This is a compact capability descriptor, not a formal JSON Schema for YAML configuration."
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    use crate::Cli;

    #[test]
    fn compact_index_is_deterministic_and_lists_nested_commands() {
        let first = schema_value(Cli::command(), &[]).unwrap();
        let second = schema_value(Cli::command(), &[]).unwrap();
        assert_eq!(first, second);
        let commands = first["commands"].as_array().unwrap();
        for expected in [
            "schema",
            "sessions.read",
            "sessions.around",
            "watch.wait",
            "tx",
        ] {
            assert!(
                commands.iter().any(|value| value == expected),
                "missing {expected}"
            );
        }
        assert!(!commands.iter().any(|value| value == "hello"));
        assert_eq!(first["limits"]["read_records_max"], 1000);
    }

    #[test]
    fn command_descriptor_comes_from_clap_and_adds_semantics() {
        let value = schema_value(Cli::command(), &["sessions.read".into()]).unwrap();
        assert_eq!(value["command"], "sessions.read");
        assert_eq!(value["mutates"], false);
        assert_eq!(value["output"]["cursor"], "sequence");
        let arguments = value["arguments"].as_array().unwrap();
        let limit = arguments.iter().find(|arg| arg["id"] == "limit").unwrap();
        assert_eq!(limit["long"], "--limit");
        assert_eq!(limit["default"], json!(["100"]));
        let time = arguments.iter().find(|arg| arg["id"] == "time").unwrap();
        assert_eq!(time["enum"], json!(["none", "relative", "absolute"]));
    }

    #[test]
    fn selector_accepts_dotted_spaced_and_split_paths() {
        let dotted = schema_value(Cli::command(), &["watch.wait".into()]).unwrap();
        let spaced = schema_value(Cli::command(), &["watch wait".into()]).unwrap();
        let split = schema_value(Cli::command(), &["watch".into(), "wait".into()]).unwrap();
        assert_eq!(dotted, spaced);
        assert_eq!(dotted, split);
    }

    #[test]
    fn error_catalog_documents_normalized_json_failures() {
        let value = schema_value(Cli::command(), &["errors".into()]).unwrap();
        assert_eq!(value["coverage"], "all_json_invocations");
        assert!(value["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "EXPECT_TIMEOUT"));
    }

    #[test]
    fn unknown_selector_is_actionable() {
        let error = schema_value(Cli::command(), &["not-a-command".into()]).unwrap_err();
        assert!(error.to_string().contains("embed-log schema"));
    }
}
