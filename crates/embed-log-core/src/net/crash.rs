//! `GET /api/sessions/{id}/crashes/{sequence}?source=SRC`: decode a crash
//! with the firmware ELF snapshot of the session (see `crate::symbols`).

use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::history::journal_path;
use super::ws_server::ServerState;

#[derive(Debug, Default, Deserialize)]
pub struct CrashQuery {
    source: String,
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        axum::Json(serde_json::json!({ "ok": false, "error": message.into() })),
    )
        .into_response()
}

/// The ELF snapshot of `source` recorded in the session manifest.
fn session_elf(session_dir: &Path, source: &str) -> Result<PathBuf, String> {
    let manifest: serde_json::Value = std::fs::read_to_string(session_dir.join("manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    crate::symbols::manifest_elf(session_dir, &manifest, source)
}

pub async fn crash_decode_handler(
    AxumPath((session_id, sequence)): AxumPath<(String, u64)>,
    Query(query): Query<CrashQuery>,
    State(state): State<ServerState>,
) -> Response {
    if query.source.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "source is required");
    }
    let journal = match journal_path(&state.logs_root, &session_id) {
        Ok(path) => path,
        Err(status) => return error(status, status.canonical_reason().unwrap_or("error")),
    };
    let Some(session_dir) = journal.parent().map(Path::to_path_buf) else {
        return error(StatusCode::NOT_FOUND, "session not found");
    };
    let elf = match session_elf(&session_dir, &query.source) {
        Ok(elf) => elf,
        Err(message) => return error(StatusCode::NOT_FOUND, message),
    };
    let elf_display = elf
        .strip_prefix(&session_dir)
        .unwrap_or(&elf)
        .to_string_lossy()
        .replace('\\', "/");
    let source = query.source.clone();
    let decoded = tokio::task::spawn_blocking(move || {
        crate::symbols::decode_crash(&journal, &source, sequence, &elf)
    })
    .await;
    match decoded {
        Ok(Ok(mut report)) => {
            // Show the session-relative snapshot, not a server path.
            report.elf = elf_display;
            axum::Json(serde_json::json!({ "ok": true, "report": report })).into_response()
        }
        Ok(Err(message)) => error(StatusCode::UNPROCESSABLE_ENTITY, message),
        Err(join) => error(StatusCode::INTERNAL_SERVER_ERROR, join.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn decodes_a_journal_crash_with_the_session_elf() {
        use super::super::ws_server::tests::test_state;
        let root = std::env::temp_dir().join(format!("embed-log-crash-api-{}", std::process::id()));
        let session = root.join("s1");
        std::fs::create_dir_all(&session).unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-m4-fault.elf");
        let symbols = crate::symbols::snapshot_elfs(&session, &[("DUT".into(), fixture)]);
        std::fs::write(
            session.join("manifest.json"),
            serde_json::json!({ "symbols": symbols }).to_string(),
        )
        .unwrap();
        let records = [
            serde_json::json!({"source_id":"DUT","sequence":1,"message":"<err> os: ***** MPU FAULT *****","event":"crash","type":"rx"}),
            serde_json::json!({"source_id":"DUT","sequence":2,"message":"<err> os: Faulting instruction address (r15/pc): 0x0000102a","type":"rx"}),
        ];
        let journal: String = records.iter().map(|r| format!("{r}\n")).collect();
        std::fs::write(session.join("combined.jsonl"), journal).unwrap();
        let (state, _) = test_state(root.clone());

        let response = crash_decode_handler(
            AxumPath(("s1".into(), 1)),
            Query(CrashQuery {
                source: "DUT".into(),
            }),
            State(state.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["report"]["addresses"][0]["label"], "pc");
        assert_eq!(
            body["report"]["addresses"][0]["frames"][0]["function"],
            "scale_sample"
        );
        assert_eq!(body["report"]["elf"], symbols["DUT"]);

        let response = crash_decode_handler(
            AxumPath(("s1".into(), 1)),
            Query(CrashQuery {
                source: "NET".into(),
            }),
            State(state.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "no ELF for NET");
        let response = crash_decode_handler(
            AxumPath(("s1".into(), 99)),
            Query(CrashQuery {
                source: "DUT".into(),
            }),
            State(state),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        std::fs::remove_dir_all(root).unwrap();
    }
}
