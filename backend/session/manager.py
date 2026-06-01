from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path

MAX_SNIPPETS = 50


class SessionManager:
    def __init__(
        self,
        *,
        session_id: str,
        session_dir: str | Path,
        tabs: list,
        source_files: dict[str, str],
        source_labels: dict[str, str],
        frontend_plugins: dict[str, dict] | None = None,
        pane_plugins: dict[str, list[dict]] | None = None,
        plugin_scripts: dict[str, str] | None = None,
        started_at: str,
        config_path: str | None,
        job_id: str | None,
        app_name: str,
        timestamp_mode: str = "absolute",
        first_log_at: str | None = None,
    ):
        self.session_id = session_id
        self.session_dir = Path(session_dir)
        self.tabs = tabs
        self.source_files = source_files
        self.source_labels = source_labels
        self.frontend_plugins = frontend_plugins or {}
        self.pane_plugins = pane_plugins or {}
        self.plugin_scripts = plugin_scripts or {}
        self.started_at = started_at
        self.config_path = config_path
        self.job_id = job_id
        self.app_name = app_name
        self.timestamp_mode = timestamp_mode
        self.first_log_at = first_log_at

        self.manifest_path = self.session_dir / "manifest.json"
        self.html_path = self.session_dir / "session.html"
        self.snippets_dir = self.session_dir / "snippets"
        self.markers_path = self.session_dir / "markers.json"

    def set_first_log_at(self, first_log_at: str | None) -> None:
        self.first_log_at = first_log_at

    def load_markers(self) -> list[dict]:
        if self.markers_path.is_file():
            try:
                data = json.loads(self.markers_path.read_text(encoding="utf-8"))
                return data.get("markers", [])
            except (json.JSONDecodeError, OSError):
                pass
        return []

    def save_markers(self, markers: list[dict]) -> None:
        self.markers_path.write_text(
            json.dumps({"session_id": self.session_id, "markers": markers}, indent=2),
            encoding="utf-8",
        )

    def build_session_info(self) -> dict:
        html_ready = self.html_path.is_file()
        html_updated_at = None
        if html_ready:
            html_updated_at = datetime.fromtimestamp(
                self.html_path.stat().st_mtime
            ).astimezone().isoformat(timespec="seconds")

        return {
            "id": self.session_id,
            "job_id": self.job_id,
            "app_name": self.app_name,
            "system_timezone": datetime.now().astimezone().tzname(),
            "dir": str(self.session_dir),
            "manifest": f"/sessions/{self.session_id}/manifest.json",
            "html": f"/sessions/{self.session_id}/session.html",
            "html_ready": html_ready,
            "html_status": "ready" if html_ready else "pending",
            "html_updated_at": html_updated_at,
            "html_error": None,
            "api": "/api/session/current",
            "started_at": self.started_at,
            "timestamp_mode": self.timestamp_mode,
            "first_log_at": self.first_log_at,
            "tabs": self.tabs,
            "pane_labels": self.source_labels,
            "frontend_plugins": self.frontend_plugins,
            "pane_plugins": self.pane_plugins,
            "sources": [
                {"name": name, "label": self.source_labels.get(name, name), "log": f"/sessions/{self.session_id}/{Path(path).name}"}
                for name, path in self.source_files.items()
            ],
        }

    def write_manifest(
        self,
        *,
        reason: str,
        exported_html: bool = False,
        html_status: str = "pending",
        html_updated_at: str | None = None,
        html_error: str | None = None,
    ) -> None:
        manifest = {
            "session_id": self.session_id,
            "session_dir": str(self.session_dir),
            "started_at": self.started_at,
            "system_timezone": datetime.now().astimezone().tzname(),
            "job_id": self.job_id,
            "config_path": self.config_path,
            "timestamp_mode": self.timestamp_mode,
            "first_log_at": self.first_log_at,
            "tabs": self.tabs,
            "pane_labels": self.source_labels,
            "frontend_plugins": self.frontend_plugins,
            "pane_plugins": self.pane_plugins,
            "plugin_scripts": self.plugin_scripts,
            "source_files": self.source_files,
            "session_html": str(self.html_path) if exported_html else None,
            "last_export_reason": reason if exported_html else None,
            "html_status": html_status,
            "html_updated_at": html_updated_at,
            "html_error": html_error,
        }
        self.manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    def save_snippet(
        self,
        text: str,
        *,
        panes: list[str],
        scope: str,
        label: str | None = None,
    ) -> str | None:
        self.snippets_dir.mkdir(parents=True, exist_ok=True)
        ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        safe_label = ""
        if label:
            safe_label = "_" + "".join(ch if ch.isalnum() or ch in ("-", "_") else "_" for ch in label)[:48]
        filename = f"snippet_{ts}{safe_label}.txt"
        path = self.snippets_dir / filename
        path.write_text(text, encoding="utf-8")

        manifest = {}
        if self.manifest_path.is_file():
            try:
                manifest = json.loads(self.manifest_path.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                manifest = {}
        snippets = manifest.get("snippets", [])
        snippets.append({
            "filename": filename,
            "path": str(path),
            "created_at": datetime.now().astimezone().isoformat(timespec="seconds"),
            "panes": panes,
            "scope": scope,
            "label": label,
            "lines": len([ln for ln in text.splitlines() if ln.strip()]),
            "bytes": len(text.encode("utf-8")),
        })
        if len(snippets) > MAX_SNIPPETS:
            snippets = snippets[-MAX_SNIPPETS:]
        manifest["snippets"] = snippets
        self.manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        return str(path)
