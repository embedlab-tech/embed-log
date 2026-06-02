import json
import tempfile
import unittest
from pathlib import Path

from backend.cli.sessions import _run_sessions_snippet


def _make_session(log_dir: Path, sid: str) -> Path:
    """Create a minimal session with snippets in the manifest and on disk."""
    sdir = log_dir / sid
    sdir.mkdir(parents=True, exist_ok=True)
    snippets_dir = sdir / "snippets"
    snippets_dir.mkdir(exist_ok=True)

    snip1 = snippets_dir / "2026-01-01T00-00-00_sel.json"
    snip1.write_text(json.dumps({"lines": ["a", "b"]}), encoding="utf-8")
    snip2 = snippets_dir / "2026-01-01T00-01-00_sel.json"
    snip2.write_text(json.dumps({"lines": ["c"]}), encoding="utf-8")

    manifest = {
        "session_id": sid,
        "source_files": {"A": str(sdir / "A.log")},
        "snippets": [
            {
                "filename": "snippets/2026-01-01T00-00-00_sel.json",
                "created_at": "2026-01-01T00:00:00Z",
                "scope": "pane",
                "panes": ["A"],
                "lines": 2,
                "label": "first",
            },
            {
                "filename": "snippets/2026-01-01T00-01-00_sel.json",
                "created_at": "2026-01-01T00:01:00Z",
                "scope": "pane",
                "panes": ["A"],
                "lines": 1,
                "label": "second",
            },
        ],
    }
    (sdir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    (sdir / "A.log").write_text("[T+00:00:00.000] boot\n", encoding="utf-8")
    return sdir


def _ns(**kw):
    """Build a simple namespace for _run_sessions_snippet."""
    return type("Args", (), kw)()


class TestSnippetList(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)
        self.sdir = _make_session(self.log_dir, "s1")

    def tearDown(self):
        self.td.cleanup()

    def test_list_shows_all_snippets(self):
        args = _ns(snippet_cmd="list", session_id="s1", json=False)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_list_json(self):
        args = _ns(snippet_cmd="list", session_id="s1", json=True)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)


class TestSnippetShow(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)
        self.sdir = _make_session(self.log_dir, "s1")

    def tearDown(self):
        self.td.cleanup()

    def test_show_last_by_default(self):
        args = _ns(snippet_cmd="show", session_id="s1", snippet_id=None, last=True, index=None)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_show_by_index(self):
        args = _ns(snippet_cmd="show", session_id="s1", snippet_id=None, last=False, index=1)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_show_by_filename_match(self):
        args = _ns(snippet_cmd="show", session_id="s1", snippet_id="00-00-00", last=False, index=None)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_show_no_match_returns_1(self):
        args = _ns(snippet_cmd="show", session_id="s1", snippet_id="nonexistent", last=False, index=None)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 1)

    def test_show_out_of_range_index_returns_1(self):
        args = _ns(snippet_cmd="show", session_id="s1", snippet_id=None, last=False, index=99)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 1)


class TestSnippetDelete(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)
        self.sdir = _make_session(self.log_dir, "s1")

    def tearDown(self):
        self.td.cleanup()

    def test_delete_by_index(self):
        args = _ns(snippet_cmd="delete", session_id="s1", index=1, all=False)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)
        remaining = json.loads((self.sdir / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(len(remaining["snippets"]), 1)

    def test_delete_all(self):
        args = _ns(snippet_cmd="delete", session_id="s1", index=None, all=True)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)
        remaining = json.loads((self.sdir / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(len(remaining["snippets"]), 0)

    def test_delete_without_flag_returns_1(self):
        args = _ns(snippet_cmd="delete", session_id="s1", index=None, all=False)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 1)


class TestSnippetMissing(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.log_dir = Path(self.td.name)

    def tearDown(self):
        self.td.cleanup()

    def test_missing_session_returns_1(self):
        args = _ns(snippet_cmd="list", session_id="nosuch", json=False)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 1)

    def test_no_snippets_returns_zero(self):
        sdir = self.log_dir / "s1"
        sdir.mkdir()
        (sdir / "manifest.json").write_text(json.dumps({"session_id": "s1", "snippets": []}), encoding="utf-8")
        args = _ns(snippet_cmd="list", session_id="s1", json=False)
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 0)

    def test_no_command_returns_1(self):
        sdir = self.log_dir / "s1"
        sdir.mkdir()
        args = _ns(snippet_cmd=None, session_id="s1")
        rc = _run_sessions_snippet(self.log_dir, args)
        self.assertEqual(rc, 1)


if __name__ == "__main__":
    unittest.main()
