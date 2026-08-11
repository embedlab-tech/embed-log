//! Shared machine-readable CLI failure contract.

use std::fmt;

use anyhow::Error;
use serde_json::{json, Value};

/// Marker error indicating that a complete JSON failure document was already
/// written to stdout and the process wrapper must not emit a second document.
#[derive(Debug)]
pub(crate) struct JsonFailureReported {
    message: String,
}

impl fmt::Display for JsonFailureReported {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for JsonFailureReported {}

pub(crate) fn report_json_failure(code: &str, message: impl Into<String>, details: Value) -> Error {
    let message = message.into();
    println!(
        "{}",
        serde_json::to_string(&json!({
            "ok": false,
            "error": {
                "code": code,
                "message": message,
                "details": details,
            }
        }))
        .expect("serialize JSON failure")
    );
    Error::new(JsonFailureReported { message })
}

pub(crate) fn is_json_failure_reported(error: &Error) -> bool {
    error.downcast_ref::<JsonFailureReported>().is_some()
}

pub(crate) fn generic_error_code(message: &str) -> &'static str {
    if message.contains("unknown schema selector") {
        "SCHEMA_SELECTOR_NOT_FOUND"
    } else if message.contains("config file")
        && (message.contains("does not exist") || message.contains("not found"))
    {
        "CONFIG_NOT_FOUND"
    } else if message.contains("unknown source") {
        "SOURCE_NOT_FOUND"
    } else if message.contains("not writable") {
        "SOURCE_NOT_WRITABLE"
    } else if message.contains("no daemon target")
        || message.contains("requires --instance")
        || message.contains("pass --instance")
    {
        "INSTANCE_REQUIRED"
    } else if message.contains("instance") && message.contains("not found") {
        "INSTANCE_NOT_FOUND"
    } else if message.contains("session") && message.contains("not found") {
        "SESSION_NOT_FOUND"
    } else if message.contains("global sequence") || message.contains("cursor") {
        "CURSOR_INVALID"
    } else {
        "COMMAND_FAILED"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_agent_failures_with_stable_fallback() {
        assert_eq!(generic_error_code("unknown source DUT"), "SOURCE_NOT_FOUND");
        assert_eq!(
            generic_error_code("source DUT is not writable"),
            "SOURCE_NOT_WRITABLE"
        );
        assert_eq!(generic_error_code("other failure"), "COMMAND_FAILED");
    }
}
