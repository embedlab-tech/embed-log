use rust_embed::RustEmbed;
use std::path::PathBuf;

/// Embedded frontend assets at compile time.
#[derive(RustEmbed)]
#[folder = "../../frontend/"]
pub struct FrontendAssets;

/// Return a filesystem path to the frontend directory, or None if unavailable.
/// In production builds with embedded assets, serve from memory instead.
pub fn resolved_frontend_dir(preferred: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(dir) = preferred {
        if dir.join("index.html").exists() {
            return Some(dir);
        }
    }
    // Fallback: try default locations
    for candidate in &["frontend", "../frontend", "../../frontend"] {
        let path = PathBuf::from(candidate);
        if path.join("index.html").exists() {
            return Some(path.canonicalize().unwrap_or(path));
        }
    }
    None
}

/// Whether this build has frontend assets embedded (always true).
pub const HAS_EMBEDDED_FRONTEND: bool = true;
