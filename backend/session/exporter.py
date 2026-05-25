from __future__ import annotations

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
        merge_script: str | Path | None = None,
        python_executable: str | None = None,
    ):
        self._session_html_path = Path(session_html_path)
        self._source_files = source_files
        self._tabs = tabs
        self._source_labels = source_labels
        self._merge_script = Path(merge_script) if merge_script else (Path(__file__).resolve().parents[2] / "utils" / "merge_logs.py")
        self._python = python_executable or sys.executable
        self._lock = threading.Lock()

    def export_html(self, reason: str) -> bool:
        with self._lock:
            if not self._merge_script.is_file():
                logging.warning("session export failed (%s): merge script not found at %s", reason, self._merge_script)
                return False

            tabs_for_export = self._tabs or [
                {"label": name, "panes": [name], "pane_labels": {name: self._source_labels.get(name, name)}} for name in self._source_files.keys()
            ]

            cmd = [self._python, str(self._merge_script)]
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

            try:
                proc = subprocess.run(cmd, capture_output=True, text=True)
                if proc.returncode != 0:
                    logging.warning("session export failed (%s): %s", reason, proc.stderr.strip())
                    return False
            except Exception as exc:
                logging.warning("session export failed (%s): %s", reason, exc)
                return False

            return True
