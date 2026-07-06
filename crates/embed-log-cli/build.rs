//! Stamps the binary with git commit + build time so `embed-log version` can
//! tell a stale install apart from a freshly built one.

fn main() {
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");

    let dirty = git_output(&["status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let sha = git_output(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let git_sha = if dirty { format!("{sha}-dirty") } else { sha };
    println!("cargo:rustc-env=EMBED_LOG_GIT_SHA={git_sha}");

    let build_time = chrono::Utc::now().format("%Y-%m-%d %H:%M:%SZ");
    println!("cargo:rustc-env=EMBED_LOG_BUILD_TIME={build_time}");
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
