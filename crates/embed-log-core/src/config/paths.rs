//! Shared path resolution used by all frontends (CLI, Tauri).

use std::path::{Path, PathBuf};

/// Resolve the logs root directory: an absolute `logs_dir` passes through
/// unchanged; a relative one resolves against the config file's parent
/// directory. Kept here so the CLI and Tauri frontends can't drift apart.
pub fn resolve_logs_root(config_path: &Path, logs_dir: &str) -> PathBuf {
    let logs = PathBuf::from(logs_dir);
    if logs.is_absolute() {
        logs
    } else {
        config_path.parent().unwrap_or(Path::new(".")).join(logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_resolves_against_config_dir() {
        assert_eq!(
            resolve_logs_root(Path::new("/etc/app/embed-log.yml"), "logs/"),
            PathBuf::from("/etc/app/logs/")
        );
    }

    #[test]
    fn absolute_passes_through() {
        let abs = if cfg!(windows) {
            r"C:\var\logs"
        } else {
            "/var/logs"
        };
        assert_eq!(
            resolve_logs_root(Path::new("/etc/app/embed-log.yml"), abs),
            PathBuf::from(abs)
        );
    }

    #[test]
    fn bare_filename_config_resolves_relative_to_cwd() {
        // parent() of a bare filename is Some(""), so the join yields "logs".
        assert_eq!(
            resolve_logs_root(Path::new("embed-log.yml"), "logs"),
            PathBuf::from("logs")
        );
    }
}
