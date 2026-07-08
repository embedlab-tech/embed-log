//! Config-path resolution: --config flag → `EMBED_LOG_CONFIG_YML_PATH` env →
//! `embed-log.yml` default.

use std::path::PathBuf;

/// Resolve the config path from (1) explicit `--config` flag, (2)
/// `EMBED_LOG_CONFIG_YML_PATH` env var, (3) `embed-log.yml` default.
pub(crate) fn resolve_config_path(config_path: Option<&PathBuf>) -> PathBuf {
    resolve_config_path_with_env(
        config_path,
        std::env::var_os("EMBED_LOG_CONFIG_YML_PATH").map(PathBuf::from),
    )
}

/// Resolve the config path with an explicit env-path override. Separated from
/// [`resolve_config_path`] so the precedence order itself is testable without
/// touching the process environment.
pub(crate) fn resolve_config_path_with_env(
    config_path: Option<&PathBuf>,
    env_path: Option<PathBuf>,
) -> PathBuf {
    config_path
        .cloned()
        .or(env_path)
        .unwrap_or_else(|| PathBuf::from("embed-log.yml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_resolution_uses_flag_then_env_then_default() {
        let flag = PathBuf::from("flag.yml");
        let env = Some(PathBuf::from("env.yml"));

        assert_eq!(
            resolve_config_path_with_env(Some(&flag), env.clone()),
            PathBuf::from("flag.yml")
        );
        assert_eq!(
            resolve_config_path_with_env(None, env),
            PathBuf::from("env.yml")
        );
        assert_eq!(
            resolve_config_path_with_env(None, None),
            PathBuf::from("embed-log.yml")
        );
    }
}
