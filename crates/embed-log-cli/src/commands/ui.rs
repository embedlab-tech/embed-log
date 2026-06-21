//! `embed-log --ui`: discover and launch the Tauri desktop binary.

use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};

/// Launch the Tauri desktop UI. Resolves the binary via env var, sibling
/// executable, or `cargo run` fallback, then blocks on the child process.
pub(crate) fn cmd_ui(config_path: Option<&PathBuf>) -> Result<()> {
    let plan = resolve_tauri_launch_plan(
        std::env::var_os("EMBED_LOG_TAURI_BIN").map(PathBuf::from),
        std::env::current_exe().ok(),
        config_path.map(PathBuf::from),
    )?;
    let mut command = tauri_command_from_plan(plan);
    let status = command.status().context("launch Tauri UI")?;
    if !status.success() {
        anyhow::bail!("Tauri UI exited with status {status}");
    }
    Ok(())
}

/// How the Tauri binary will be invoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TauriLaunchPlan {
    /// Run a specific binary path directly.
    Direct { program: PathBuf, args: Vec<String> },
    /// `cargo run --package embed-log-tauri` from a source checkout.
    Cargo { args: Vec<String> },
}

/// Resolve how to launch the Tauri UI, in priority order:
/// 1. `EMBED_LOG_TAURI_BIN` env var → direct.
/// 2. `embed-log-tauri` sibling of the current executable → direct.
/// 3. `Cargo.toml` in cwd → `cargo run`.
/// 4. Otherwise: error.
pub(crate) fn resolve_tauri_launch_plan(
    env_bin: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    config_path: Option<PathBuf>,
) -> Result<TauriLaunchPlan> {
    let config_args = config_path
        .as_ref()
        .map(|path| vec!["--config".to_string(), path.display().to_string()])
        .unwrap_or_default();

    if let Some(program) = env_bin {
        return Ok(TauriLaunchPlan::Direct {
            program,
            args: config_args,
        });
    }

    if let Some(current_exe) = current_exe {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(if cfg!(windows) {
                "embed-log-tauri.exe"
            } else {
                "embed-log-tauri"
            });
            if candidate.exists() {
                return Ok(TauriLaunchPlan::Direct {
                    program: candidate,
                    args: config_args,
                });
            }
        }
    }

    if std::path::Path::new("Cargo.toml").exists() {
        let mut args = vec![
            "run".to_string(),
            "--quiet".to_string(),
            "--package".to_string(),
            "embed-log-tauri".to_string(),
            "--bin".to_string(),
            "embed-log-tauri".to_string(),
            "--".to_string(),
        ];
        args.extend(config_args);
        return Ok(TauriLaunchPlan::Cargo { args });
    }

    anyhow::bail!(
        "could not locate embed-log-tauri; set EMBED_LOG_TAURI_BIN or install the Tauri binary next to embed-log"
    )
}

/// Build the `std::process::Command` for a resolved launch plan.
pub(crate) fn tauri_command_from_plan(plan: TauriLaunchPlan) -> ProcessCommand {
    match plan {
        TauriLaunchPlan::Direct { program, args } => {
            let mut command = ProcessCommand::new(program);
            command.args(args);
            command
        }
        TauriLaunchPlan::Cargo { args } => {
            let mut command = ProcessCommand::new("cargo");
            command.args(args);
            command
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_bin_overrides_everything() {
        let plan = resolve_tauri_launch_plan(
            Some(PathBuf::from("/tmp/embed-log-tauri")),
            None,
            Some(PathBuf::from("desktop.yml")),
        )
        .unwrap();
        assert_eq!(
            plan,
            TauriLaunchPlan::Direct {
                program: PathBuf::from("/tmp/embed-log-tauri"),
                args: vec!["--config".to_string(), "desktop.yml".to_string()],
            }
        );
    }

    #[test]
    fn env_bin_without_config_has_no_config_args() {
        let plan =
            resolve_tauri_launch_plan(Some(PathBuf::from("/opt/tauri")), None, None).unwrap();
        assert_eq!(
            plan,
            TauriLaunchPlan::Direct {
                program: PathBuf::from("/opt/tauri"),
                args: vec![],
            }
        );
    }
}
