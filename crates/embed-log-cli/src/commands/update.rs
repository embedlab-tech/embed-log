//! Safe, best-effort updates from the project's GitHub Release artifacts.
//!
//! Normal CLI operation never depends on this module succeeding: update hints
//! run on a detached thread and ignore every error.

#[cfg(windows)]
use std::process::Command;
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};

const REPOSITORY: &str = "embedlab-tech/embed-log";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MARKER_NAME: &str = ".embed-log-install";

#[derive(Debug, Deserialize)]
struct Release {
    version: String,
    assets: std::collections::BTreeMap<String, ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    archive: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct InstallMarker {
    repository: String,
    target: String,
    executable: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateCache {
    checked_at: u64,
    available_version: String,
}

pub(crate) fn cmd_update(check_only: bool) -> Result<()> {
    let release = fetch_release()?;
    let current = current_version()?;
    let available = parse_version(&release.version)?;

    if available <= current {
        println!("embed-log {} is up to date.", current);
        return Ok(());
    }
    if check_only {
        println!("Update available: v{current} -> v{available}");
        return Ok(());
    }

    let exe = std::env::current_exe().context("could not determine the installed executable")?;
    let target = target();
    verify_managed_install(&exe, target)?;
    let asset = release.assets.get(target).with_context(|| {
        format!("release v{available} does not contain an artifact for {target}")
    })?;
    validate_asset(asset)?;

    println!("Updating embed-log: v{current} -> v{available}");
    let archive = download_asset(&asset.archive)?;
    let replacement = (|| {
        verify_sha256(&archive, &asset.sha256)?;
        extract_binary(&archive, &asset.archive, &exe)
    })();
    fs::remove_file(&archive).ok();
    let replacement = replacement?;
    replace_executable(&replacement, &exe)?;
    println!("embed-log was updated to v{available}.");
    Ok(())
}

/// Starts an update check that cannot delay or fail the command it accompanies.
pub(crate) fn spawn_update_hint() {
    if std::env::var_os("EMBED_LOG_NO_UPDATE_CHECK").is_some() {
        return;
    }
    std::thread::spawn(|| {
        // A cached result deliberately does not produce a hint: this makes an
        // offline invocation completely silent, even after a prior update check.
        let available = match freshly_fetched_version_for_hint() {
            Ok(Some(version)) => version,
            Ok(None) | Err(_) => return,
        };
        if let (Ok(current), Ok(available)) = (current_version(), parse_version(&available)) {
            if available > current {
                eprintln!(
                    "A newer embed-log version (v{available}) is available. Run: embed-log update"
                );
            }
        }
    });
}

fn freshly_fetched_version_for_hint() -> Result<Option<String>> {
    if let Some(cache) = read_cache()? {
        if now_secs().saturating_sub(cache.checked_at) < CACHE_TTL.as_secs() {
            return Ok(None);
        }
    }
    let release = fetch_release()?;
    write_cache(&UpdateCache {
        checked_at: now_secs(),
        available_version: release.version.clone(),
    })?;
    Ok(Some(release.version))
}

fn fetch_release() -> Result<Release> {
    let base = std::env::var("EMBED_LOG_UPDATE_BASE_URL")
        .unwrap_or_else(|_| format!("https://github.com/{REPOSITORY}/releases/latest/download"));
    let url = format!("{}/release.json", base.trim_end_matches('/'));
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .user_agent(format!("embed-log/{}", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let release: Release = response.json()?;
    parse_version(&release.version)?;
    Ok(release)
}

fn current_version() -> Result<Version> {
    parse_version(env!("CARGO_PKG_VERSION"))
}

fn parse_version(value: &str) -> Result<Version> {
    Version::parse(value.trim_start_matches('v'))
        .with_context(|| format!("invalid release version: {value}"))
}

fn target() -> &'static str {
    env!("EMBED_LOG_TARGET")
}

fn verify_managed_install(exe: &Path, target: &str) -> Result<()> {
    let parent = exe
        .parent()
        .context("installed executable has no parent directory")?;
    let marker_path = parent.join(MARKER_NAME);
    let marker = fs::read_to_string(&marker_path).with_context(|| {
        format!(
            "this installation is not managed by embed-log ({})",
            marker_path.display()
        )
    })?;
    let marker: InstallMarker =
        serde_json::from_str(&marker).context("invalid embed-log install marker")?;
    let marked_executable = fs::canonicalize(&marker.executable)
        .context("install marker refers to a missing executable")?;
    let executable =
        fs::canonicalize(exe).context("installed executable is no longer available")?;
    if marker.repository != REPOSITORY || marker.target != target || marked_executable != executable
    {
        bail!("this installation is not managed by this embed-log release; update it using its package manager or reinstall with the official installer")
    }
    Ok(())
}

fn validate_asset(asset: &ReleaseAsset) -> Result<()> {
    if asset.archive.contains('/')
        || asset.archive.contains('\\')
        || asset.sha256.len() != 64
        || !asset.sha256.bytes().all(|c| c.is_ascii_hexdigit())
    {
        bail!("release metadata contains an invalid archive or checksum")
    }
    Ok(())
}

fn download_asset(name: &str) -> Result<PathBuf> {
    let base = std::env::var("EMBED_LOG_UPDATE_BASE_URL")
        .unwrap_or_else(|_| format!("https://github.com/{REPOSITORY}/releases/latest/download"));
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .get(format!("{}/{name}", base.trim_end_matches('/')))
        .send()?
        .error_for_status()?;
    let path = std::env::temp_dir().join(format!("embed-log-update-{}-{name}", std::process::id()));
    let mut file = fs::File::create(&path)?;
    let mut reader = response;
    io::copy(&mut reader, &mut file)?;
    Ok(path)
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    // SHA-256 is intentionally verified by the release metadata before any
    // archive contents are extracted or installed.
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("downloaded archive checksum does not match release metadata")
    }
    Ok(())
}

fn extract_binary(archive: &Path, archive_name: &str, exe: &Path) -> Result<PathBuf> {
    let parent = exe
        .parent()
        .context("installed executable has no parent directory")?;
    let output = parent.join(format!(".embed-log-update-{}", std::process::id()));
    let binary_name = if cfg!(windows) {
        "embed-log.exe"
    } else {
        "embed-log"
    };
    let bytes = if archive_name.ends_with(".tar.gz") {
        let file = fs::File::open(archive)?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let entry = tar.entries()?.find_map(|entry| match entry {
            Ok(mut entry)
                if entry
                    .path()
                    .ok()
                    .is_some_and(|p| p == Path::new(binary_name)) =>
            {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).ok().map(|_| bytes)
            }
            _ => None,
        });
        entry.context("release archive does not contain embed-log")?
    } else if archive_name.ends_with(".zip") {
        let file = fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        let mut entry = zip
            .by_name(binary_name)
            .context("release archive does not contain embed-log")?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        bytes
    } else {
        bail!("unsupported release archive format")
    };
    fs::write(&output, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
    }
    Ok(output)
}

#[cfg(not(windows))]
fn replace_executable(replacement: &Path, exe: &Path) -> Result<()> {
    fs::rename(replacement, exe).context("could not replace installed executable")
}

#[cfg(windows)]
fn replace_executable(replacement: &Path, exe: &Path) -> Result<()> {
    // Windows locks a running executable. The detached shell waits briefly for
    // this process to exit, then copies over the destination and removes the
    // staged file. `move` cannot reliably replace an existing destination on
    // Windows, whereas `copy /Y` is an explicit overwrite.
    Command::new("cmd")
        .args([
            "/C",
            &format!(
                "ping 127.0.0.1 -n 2 > nul & copy /Y \"{}\" \"{}\" > nul && del /F /Q \"{}\" > nul",
                replacement.display(),
                exe.display(),
                replacement.display(),
            ),
        ])
        .spawn()
        .context("could not start Windows update helper")?;
    println!("The update will finish after embed-log exits.");
    Ok(())
}

fn cache_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("embed-log").join("update.json"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .map(|p| p.join("embed-log").join("update.json"))
    }
}

fn read_cache() -> Result<Option<UpdateCache>> {
    let Some(path) = cache_path() else {
        return Ok(None);
    };
    match fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_cache(cache: &UpdateCache) -> Result<()> {
    let Some(path) = cache_path() else {
        return Ok(());
    };
    let parent = path.parent().expect("cache path has parent");
    fs::create_dir_all(parent)?;
    fs::write(path, serde_json::to_vec(cache)?)?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_versions_accept_tags_and_compare_semantically() {
        assert_eq!(parse_version("v1.10.0").unwrap(), Version::new(1, 10, 0));
        assert!(parse_version("1.10.0").unwrap() > parse_version("1.9.9").unwrap());
        assert!(parse_version("not-a-version").is_err());
    }

    #[test]
    fn release_asset_rejects_paths_and_invalid_checksums() {
        let valid = ReleaseAsset {
            archive: "embed-log-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            sha256: "a".repeat(64),
        };
        assert!(validate_asset(&valid).is_ok());
        assert!(validate_asset(&ReleaseAsset {
            archive: "../embed-log.tar.gz".to_string(),
            sha256: "a".repeat(64),
        })
        .is_err());
        assert!(validate_asset(&ReleaseAsset {
            archive: valid.archive,
            sha256: "not-a-checksum".to_string(),
        })
        .is_err());
    }
}
