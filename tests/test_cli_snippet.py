import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backend.cli import _run_sessions_snippet


def _make_session(log_dir: Path, sid: str) -> Path:
    """Create a minimal session with snippets in the manifest and on disk."""
    sdir = log_dir / sid
    sdir.mkdir(parents=True, exist_ok=True)
    snippets_dir = sdir / "snippets"
    snippets_dir.mkdir(exist_ok=True)

    snippet_specs = [
        ("2026-01-01_00-00-01-alpha.log", "alpha content\n", "exact", ["SENSOR_A"]),
        ("2026-01-01_00-00-02-beta.log", "beta content\nline 2\n", "context", ["SENSOR_A", "SENSOR_B"]),
        ("2026-01-01_00-00-03-gamma.log", "gamma content\n", "context-selected", ["SENSOR_C"]),
    ]
    snippets = []
    for i, (filename, text, scope, panes) in enumerate(snippet_specs):
        (snippets_dir / filename).write_text(text, encoding="utf-8")
        snippets.append({
            "file": f"snippets/{filename}",
            "label": filename.split("-", 3)[-1].replace(".log", ""),
            "scope": scope,
            "panes": panes,
            "line_count": text.count("\n") + 1,
            "saved_at": f"2026-01-01T00:00:{i:02d}:00+00:00",
        })

    manifest = {
        "session_id": sid,
        "snippets": snippets,
    }
    (sdir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return sdir


def _ns(**kw):
    """Build a simple namespace for _run_sessions_snippet."""
    return type("Args", (), kw)()


class TestSnippetList(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)
        self.sid = "2026-01-01_test-session"
        _make_session(self.log_dir, self.sid)

    def tearDown(self):
        self.td.cleanup()

    def test_list_exits_zero(self):
        ns = _ns(snippet_cmd="list", session_id=self.sid, json=False, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

    def test_list_json(self):
        ns = _ns(snippet_cmd="list", session_id=self.sid, json=True, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

    def test_show_last_default(self):
        ns = _ns(snippet_cmd="show", session_id=self.sid, snippet_id=None,
                 last=False, index=None, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

    def test_show_with_index(self):
        ns = _ns(snippet_cmd="show", session_id=self.sid, snippet_id=None,
                 last=False, index=2, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

    def test_show_with_filename_prefix(self):
        ns = _ns(snippet_cmd="show", session_id=self.sid, snippet_id="alpha",
                 last=False, index=None, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

    def test_show_out_of_range_index(self):
        ns = _ns(snippet_cmd="show", session_id=self.sid, snippet_id=None,
                 last=False, index=99, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 1)

    def test_show_no_match(self):
        ns = _ns(snippet_cmd="show", session_id=self.sid, snippet_id="nonexistent",
                 last=False, index=None, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 1)

    def test_delete_by_index(self):
        ns = _ns(snippet_cmd="delete", session_id=self.sid, all=False,
                 index=1, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

        manifest = json.loads((self.log_dir / self.sid / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(len(manifest["snippets"]), 2)
        self.assertFalse((self.log_dir / self.sid / "snippets" / "2026-01-01_00-00-01-alpha.log").exists())

    def test_delete_all(self):
        ns = _ns(snippet_cmd="delete", session_id=self.sid, all=True,
                 index=None, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

        manifest = json.loads((self.log_dir / self.sid / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(len(manifest["snippets"]), 0)
        self.assertFalse((self.log_dir / self.sid / "snippets").exists())

    def test_delete_without_flags(self):
        ns = _ns(snippet_cmd="delete", session_id=self.sid, all=False,
                 index=None, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 1)

    def test_no_snippets_returns_zero(self):
        empty_sid = "empty-session"
        sdir = self.log_dir / empty_sid
        sdir.mkdir(exist_ok=True)
        (sdir / "manifest.json").write_text(json.dumps({"session_id": empty_sid}), encoding="utf-8")
        ns = _ns(snippet_cmd="list", session_id=empty_sid, json=False, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 0)

    def test_session_not_found(self):
        ns = _ns(snippet_cmd="list", session_id="no-such-session", json=False,
                 log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 1)

    def test_no_command(self):
        ns = _ns(snippet_cmd="", session_id=self.sid, json=False, log_dir=str(self.log_dir))
        rc = _run_sessions_snippet(self.log_dir, ns)
        self.assertEqual(rc, 1)


if __name__ == "__main__":
    unittest.main()
