//! Print the version-matched canonical agent skill embedded in the binary.

use anyhow::Result;
use serde_json::json;

use super::schema::SCHEMA_VERSION;

pub(crate) const EMBED_LOG_SKILL: &str = include_str!("../../../../skills/embed-log/SKILL.md");

pub(crate) fn cmd_skill(json_output: bool) -> Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "schema_version": SCHEMA_VERSION,
                "embed_log_version": env!("CARGO_PKG_VERSION"),
                "format": "markdown",
                "content": EMBED_LOG_SKILL,
            }))?
        );
    } else {
        print!("{EMBED_LOG_SKILL}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_is_canonical_and_complete() {
        assert_eq!(
            EMBED_LOG_SKILL,
            include_str!("../../../../skills/embed-log/SKILL.md")
        );
        assert!(EMBED_LOG_SKILL.starts_with("---\ndescription:"));
        assert!(EMBED_LOG_SKILL.contains("embed-log schema"));
        assert!(EMBED_LOG_SKILL.contains("embed-log tx"));
        assert!(EMBED_LOG_SKILL.ends_with('\n'));
    }
}
