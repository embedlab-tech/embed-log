from __future__ import annotations

import json
import logging
import subprocess
import sys
import threading
from pathlib import Path


class SessionExporter:
    def __init__(
        self,
        *,
        session_html_path: str | Path,
        source_files: dict[str, str],
        tabs: list,
        source_labels: dict[str, str],
        frontend_plugins: dict[str, dict] | None = None,
        pane_plugins: dict[str, list[dict]] | None = None,
        plugin_scripts: dict[str, str] | None = None,
        timestamp_mode: str = "absolute",
        first_log_at: str | None = None,
        merge_script: str | Path | None = None,
        python_executable: str | None = None,
    ):
        self._session_html_path = Path(session_html_path)
        self._source_files = source_files
        self._tabs = tabs
        self._source_labels = source_labels
        self._frontend_plugins = frontend_plugins or {}
        self._pane_plugins = pane_plugins or {}
        self._plugin_scripts = plugin_scripts or {}
        self._timestamp_mode = timestamp_mode
        self._first_log_at = first_log_at
        self._merge_script = Path(merge_script) if merge_script else (Path(__file__).resolve().parents[2] / "utils" / "merge_logs.py")
        self._python = python_executable or sys.executable
        self._lock = threading.Lock()

    def set_first_log_at(self, first_log_at: str | None) -> None:
        self._first_log_at = first_log_at

    def export_html(self, reason: str) -> bool:
        with self._lock:
            if not self._merge_script.is_file():
                logging.warning("session export failed (%s): merge script not found at %s", reason, self._merge_script)
                return False

            tabs_for_export = self._tabs or [
                {"label": name, "panes": [name], "pane_labels": {name: self._source_labels.get(name, name)}} for name in self._source_files.keys()
            ]

            cmd = [self._python, str(self._merge_script)]
            if self._timestamp_mode:
                cmd.extend(["--timestamp-mode", self._timestamp_mode])
            if self._first_log_at:
                cmd.extend(["--first-log-at", self._first_log_at])
            if self._frontend_plugins:
                cmd.extend(["--frontend-plugins-json", json.dumps(self._frontend_plugins, ensure_ascii=False)])
            if self._pane_plugins:
                cmd.extend(["--pane-plugins-json", json.dumps(self._pane_plugins, ensure_ascii=False)])
            if self._plugin_scripts:
                cmd.extend(["--plugin-scripts-json", json.dumps(self._plugin_scripts, ensure_ascii=False)])
            for tab in tabs_for_export:
                cmd.extend(["--tab", tab["label"]])
                pane_labels = tab.get("pane_labels", {})
                for pane in tab.get("panes", []):
                    file_path = self._source_files.get(pane)
                    if not file_path:
                        continue
                    pane_label = pane_labels.get(pane, self._source_labels.get(pane, pane))
                    cmd.extend([f"{pane}={pane_label}", file_path])
            cmd.extend(["--output", str(self._session_html_path)])
            markers_path = self._session_html_path.parent / "markers.json"
            if markers_path.is_file():
                cmd.extend(["--markers-file", str(markers_path)])
            try:
                proc = subprocess.run(cmd, capture_output=True, text=True)
                if proc.returncode != 0:
                    logging.warning("session export failed (%s): %s", reason, proc.stderr.strip())
                    return False
            except Exception as exc:
                logging.warning("session export failed (%s): %s", reason, exc)
                return False

            return True
