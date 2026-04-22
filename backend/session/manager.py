from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import Optional


class SessionManager:
    def __init__(
        self,
        *,
        session_id: str,
        session_dir: str | Path,
        tabs: list,
        source_files: dict[str, str],
        started_at: str,
        config_path: Optional[str],
        job_id: Optional[str],
        app_name: str,
    ):
        self.session_id = session_id
        self.session_dir = Path(session_dir)
        self.tabs = tabs
        self.source_files = source_files
        self.started_at = started_at
        self.config_path = config_path
        self.job_id = job_id
        self.app_name = app_name

        self.manifest_path = self.session_dir / "manifest.json"
        self.html_path = self.session_dir / "session.html"

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
            "dir": str(self.session_dir),
            "manifest": f"/sessions/{self.session_id}/manifest.json",
            "html": f"/sessions/{self.session_id}/session.html",
            "html_ready": html_ready,
            "html_status": "ready" if html_ready else "pending",
            "html_updated_at": html_updated_at,
            "html_error": None,
            "api": "/api/session/current",
            "tabs": self.tabs,
            "sources": [
                {"name": name, "log": f"/sessions/{self.session_id}/{Path(path).name}"}
                for name, path in self.source_files.items()
            ],
        }

    def write_manifest(
        self,
        *,
        reason: str,
        exported_html: bool = False,
        html_status: str = "pending",
        html_updated_at: Optional[str] = None,
        html_error: Optional[str] = None,
    ) -> None:
        manifest = {
            "session_id": self.session_id,
            "session_dir": str(self.session_dir),
            "started_at": self.started_at,
            "job_id": self.job_id,
            "config_path": self.config_path,
            "tabs": self.tabs,
            "source_files": self.source_files,
            "session_html": str(self.html_path) if exported_html else None,
            "last_export_reason": reason if exported_html else None,
            "html_status": html_status,
            "html_updated_at": html_updated_at,
            "html_error": html_error,
        }
        self.manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
