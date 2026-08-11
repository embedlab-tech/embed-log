//! Canonical HTML export for the active daemon session.

use std::time::Duration;

use anyhow::{Context, Result};

use super::daemon::{http_post_json_with_timeout, resolve_mutating_endpoint, InstanceRecord};

const EXPORT_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) fn cmd_export(instance: Option<&str>, url: Option<&str>, json: bool) -> Result<()> {
    let (record, endpoint) = resolve_mutating_endpoint(instance, url)?;
    let export = http_post_json_with_timeout(
        &endpoint,
        "/api/session/export",
        &serde_json::json!({}),
        EXPORT_TIMEOUT,
    )
    .context("export active session HTML")?;

    let output = serde_json::json!({
        "ok": true,
        "instance": record,
        "endpoint": endpoint,
        "export": export,
    });
    if json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        print_human_result(record.as_ref(), &endpoint, &export);
    }
    Ok(())
}

fn print_human_result(record: Option<&InstanceRecord>, endpoint: &str, export: &serde_json::Value) {
    if let Some(record) = record {
        println!("instance {}", record.instance);
    }
    println!("  endpoint: {endpoint}");
    println!(
        "  session:  {}",
        export
            .pointer("/session/id")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
    );
    println!(
        "  html:     {}",
        export
            .get("html_path")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
    );
}
