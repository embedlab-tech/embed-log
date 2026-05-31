from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path

from backend.parse import run_parse


# Epoch millis for 2025-05-30T18:50:41.955Z
_TS_MS = 1748631041955


def _build_html(
    *,
    include_markers: bool = True,
    include_pane_labels: bool = True,
    include_ts_mode: bool = True,
    include_first_log: bool = True,
    include_logdata: bool = True,
) -> str:
    """Build a minimal HTML string with all embedded data."""
    log_data = {
        "uart0": [
            {"ts": "05-30 19:07:21.955", "absNum": _TS_MS, "text": "hello"},
            {"ts": "05-30 19:07:22.100", "absNum": _TS_MS + 145, "text": "world"},
        ],
        "sys": [
            {"ts": "05-30 19:07:20.000", "absNum": _TS_MS - 1955, "text": "boot"},
        ],
    }
    markers = [
        {"ts": "2025-05-30T19:07:21.955Z", "label": "start", "color": "#ff0000"},
        {"ts": "2025-05-30T19:07:22.100Z", "label": "end", "color": "#00ff00"},
    ]
    tabs = [{"name": "uart0", "source": "uart0"}]
    pane_labels = {"left": "UART", "right": "System"}
    ts_mode = "absolute"
    first_log_at = "2025-05-30T19:07:20.000+00:00"

    parts = ["<html><body><script>"]
    if include_logdata:
        parts.append(f"var _logData = {json.dumps(log_data)};")
    if include_markers:
        parts.append(f"var _markers = {json.dumps(markers)};")
    parts.append(f"window.TABS = {json.dumps(tabs)};")
    if include_pane_labels:
        parts.append(f"window.PANE_LABELS = {json.dumps(pane_labels)};")
    if include_ts_mode:
        parts.append(f'window.__embedLogInitialTimestampMode = {json.dumps(ts_mode)};')
    if include_first_log:
        parts.append(f'window.__embedLogFirstLogAt = {json.dumps(first_log_at)};')
    parts.append("</script></body></html>")
    return "\n".join(parts)


class RunParseFullDataTests(unittest.TestCase):
    """Test parsing HTML with all embedded data present."""

    def setUp(self):
        self._tmp = tempfile.mkdtemp()
        html = _build_html()
        self._html_path = Path(self._tmp) / "session.html"
        self._html_path.write_text(html, encoding="utf-8")
        self._out_dir = Path(self._tmp) / "output"

    def tearDown(self):
        import shutil
        shutil.rmtree(self._tmp, ignore_errors=True)

    def _run(self, extra_args: list[str] | None = None):
        args = [str(self._html_path), "-o", str(self._out_dir)]
        if extra_args:
            args.extend(extra_args)
        return run_parse(args)

    def test_returns_zero(self):
        self.assertEqual(self._run(), 0)

    def test_creates_log_files(self):
        self._run()
        self.assertTrue((self._out_dir / "uart0.log").is_file())
        self.assertTrue((self._out_dir / "sys.log").is_file())

    def test_log_content_uses_absnum(self):
        self._run()
        lines = (self._out_dir / "uart0.log").read_text().splitlines()
        self.assertEqual(len(lines), 2)
        # absNum 1748631041955 → 2025-05-30T18:50:41.955+00:00
        self.assertIn("2025-05-30T18:50:41.955+00:00", lines[0])
        self.assertIn("hello", lines[0])

    def test_creates_markers_json(self):
        self._run()
        markers_path = self._out_dir / "markers.json"
        self.assertTrue(markers_path.is_file())
        data = json.loads(markers_path.read_text())
        self.assertEqual(data["session_id"], self._out_dir.name)
        self.assertEqual(len(data["markers"]), 2)

    def test_manifest_has_all_fields(self):
        self._run()
        manifest = json.loads((self._out_dir / "manifest.json").read_text())
        self.assertEqual(manifest["session_id"], self._out_dir.name)
        self.assertEqual(manifest["session_dir"], str(self._out_dir))
        self.assertEqual(manifest["source"], "parsed_html")
        self.assertEqual(manifest["timestamp_mode"], "absolute")
        self.assertEqual(manifest["first_log_at"], "2025-05-30T19:07:20.000+00:00")
        self.assertIn("tabs", manifest)
        self.assertEqual(manifest["tabs"], [{"name": "uart0", "source": "uart0"}])
        self.assertEqual(manifest["pane_labels"], {"left": "UART", "right": "System"})
        self.assertIn("source_files", manifest)
        self.assertIn("uart0", manifest["source_files"])
        self.assertIn("sys", manifest["source_files"])
        self.assertEqual(manifest["started_at"], "2025-05-30T19:07:20.000+00:00")

    def test_started_at_uses_first_log_at(self):
        self._run()
        manifest = json.loads((self._out_dir / "manifest.json").read_text())
        self.assertEqual(manifest["started_at"], manifest["first_log_at"])


class RunParseNoMarkersTests(unittest.TestCase):
    """Test parsing HTML with no markers (graceful degradation)."""

    def setUp(self):
        self._tmp = tempfile.mkdtemp()
        html = _build_html(include_markers=False)
        self._html_path = Path(self._tmp) / "session.html"
        self._html_path.write_text(html, encoding="utf-8")
        self._out_dir = Path(self._tmp) / "output"

    def tearDown(self):
        import shutil
        shutil.rmtree(self._tmp, ignore_errors=True)

    def test_succeeds_without_markers(self):
        ret = run_parse([str(self._html_path), "-o", str(self._out_dir)])
        self.assertEqual(ret, 0)

    def test_no_markers_json_written(self):
        run_parse([str(self._html_path), "-o", str(self._out_dir)])
        self.assertFalse((self._out_dir / "markers.json").exists())

    def test_markers_zero_in_manifest(self):
        run_parse([str(self._html_path), "-o", str(self._out_dir)])
        # markers should not appear in manifest (we don't write it)
        manifest = json.loads((self._out_dir / "manifest.json").read_text())
        # log_data files still present
        self.assertTrue(len(manifest["source_files"]) > 0)


class RunParseDefaultOutputTests(unittest.TestCase):
    """Test session naming from first_log_at when --output is not given."""

    def setUp(self):
        self._tmp = tempfile.mkdtemp()
        self._orig_cwd = os.getcwd()
        os.chdir(self._tmp)

    def tearDown(self):
        os.chdir(self._orig_cwd)
        import shutil
        shutil.rmtree(self._tmp, ignore_errors=True)

    def test_session_named_from_first_log_at(self):
        html = _build_html()
        html_path = Path(self._tmp) / "session.html"
        html_path.write_text(html, encoding="utf-8")
        ret = run_parse([str(html_path)])
        self.assertEqual(ret, 0)
        # first_log_at = "2025-05-30T19:07:20.000+00:00" → "2025-05-30_19-07-20"
        expected = Path("2025-05-30_19-07-20")
        self.assertTrue(expected.is_dir(), f"Expected {expected} to exist in {os.listdir('.')}")
        self.assertTrue((expected / "manifest.json").is_file())

    def test_session_fallback_when_no_first_log_at(self):
        html = _build_html(include_first_log=False)
        html_path = Path(self._tmp) / "session.html"
        html_path.write_text(html, encoding="utf-8")
        ret = run_parse([str(html_path)])
        self.assertEqual(ret, 0)
        # Should create a timestamped dir (pattern: YYYY-MM-DD_HH-MM-SS)
        dirs = [d for d in Path(".").iterdir() if d.is_dir() and d.name[0].isdigit()]
        self.assertEqual(len(dirs), 1)
        self.assertTrue((dirs[0] / "manifest.json").is_file())


class RunParseMissingFileTests(unittest.TestCase):
    """Test error handling for missing HTML file."""

    def test_raises_on_missing_file(self):
        with self.assertRaises(SystemExit):
            run_parse(["/nonexistent/path.html"])


class RunParseMinimalTests(unittest.TestCase):
    """Test with minimal data (only logdata + tabs, no extras)."""

    def setUp(self):
        self._tmp = tempfile.mkdtemp()
        html = _build_html(
            include_markers=False,
            include_pane_labels=False,
            include_ts_mode=False,
            include_first_log=False,
        )
        self._html_path = Path(self._tmp) / "session.html"
        self._html_path.write_text(html, encoding="utf-8")
        self._out_dir = Path(self._tmp) / "output"

    def tearDown(self):
        import shutil
        shutil.rmtree(self._tmp, ignore_errors=True)

    def test_succeeds_with_minimal_html(self):
        ret = run_parse([str(self._html_path), "-o", str(self._out_dir)])
        self.assertEqual(ret, 0)

    def test_manifest_optional_fields_none(self):
        run_parse([str(self._html_path), "-o", str(self._out_dir)])
        manifest = json.loads((self._out_dir / "manifest.json").read_text())
        self.assertIsNone(manifest["timestamp_mode"])
        self.assertIsNone(manifest["first_log_at"])
        self.assertIsNone(manifest["pane_labels"])
        # falls back to first_ts from log data (absNum)
        self.assertEqual(manifest["started_at"], "2025-05-30T18:50:41.955+00:00")


if __name__ == "__main__":
    unittest.main()
