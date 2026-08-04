//! Print the version-matched canonical agent skill embedded in the binary.

use anyhow::Result;
use clap::ValueEnum;
use serde_json::json;

use super::schema::SCHEMA_VERSION;

pub(crate) const LIVE_SKILL: &str = include_str!("../../../../skills/embed-log-live/SKILL.md");
pub(crate) const RECORDED_SKILL: &str =
    include_str!("../../../../skills/embed-log-recorded/SKILL.md");

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum SkillKind {
    Live,
    Recorded,
}

impl SkillKind {
    fn name(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Recorded => "recorded",
        }
    }

    fn content(self) -> &'static str {
        match self {
            Self::Live => LIVE_SKILL,
            Self::Recorded => RECORDED_SKILL,
        }
    }
}

pub(crate) fn cmd_skill(kind: SkillKind, json_output: bool) -> Result<()> {
    let content = kind.content();
    if json_output {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "schema_version": SCHEMA_VERSION,
                "embed_log_version": env!("CARGO_PKG_VERSION"),
                "skill": kind.name(),
                "format": "markdown",
                "content": content,
            }))?
        );
    } else {
        print!("{content}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_is_canonical_and_complete() {
        assert_eq!(
            LIVE_SKILL,
            include_str!("../../../../skills/embed-log-live/SKILL.md")
        );
        assert_eq!(
            RECORDED_SKILL,
            include_str!("../../../../skills/embed-log-recorded/SKILL.md")
        );
        for skill in [LIVE_SKILL, RECORDED_SKILL] {
            assert!(skill.starts_with("---\ndescription:"));
            assert!(skill.contains("embed-log schema"));
            assert!(skill.ends_with('\n'));
        }
        assert!(LIVE_SKILL.contains("embed-log tx"));
        assert!(RECORDED_SKILL.contains("embed-log sessions"));
    }
}
