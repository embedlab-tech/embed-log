//! Print the version-matched canonical agent skill embedded in the binary.

use anyhow::Result;
use serde_json::json;

use super::schema::SCHEMA_VERSION;

pub(crate) const SKILL: &str = include_str!("../../../../skills/embed-log/SKILL.md");

pub(crate) fn cmd_skill(json_output: bool) -> Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "schema_version": SCHEMA_VERSION,
                "embed_log_version": env!("CARGO_PKG_VERSION"),
                "skill": "embed-log",
                "format": "markdown",
                "content": SKILL,
            }))?
        );
    } else {
        print!("{SKILL}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_is_canonical_and_complete() {
        assert_eq!(SKILL, include_str!("../../../../skills/embed-log/SKILL.md"));
        assert_eq!(SKILL.lines().next(), Some("---"));
        assert!(SKILL
            .lines()
            .nth(1)
            .is_some_and(|line| line.starts_with("description:")));
        assert!(SKILL.contains("embed-log schema"));
        assert!(SKILL.contains("embed-log sessions read"));
        assert!(!SKILL.contains("watch"));
        assert!(SKILL.ends_with('\n'));
    }
}
