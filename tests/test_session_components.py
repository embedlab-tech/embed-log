import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backend.session import SessionExporter, SessionManager


class SessionManagerTests(unittest.TestCase):
    def test_build_info_and_manifest(self):
        with tempfile.TemporaryDirectory() as td:
            session_dir = Path(td) / "2026-01-01_00-00-00"
            session_dir.mkdir(parents=True, exist_ok=True)
            source_files = {"A": str(session_dir / "A.log")}

            mgr = SessionManager(
                session_id="2026-01-01_00-00-00",
                session_dir=session_dir,
                tabs=[{"label": "T", "panes": ["A"]}],
                source_files=source_files,
                source_labels={"A": "READER"},
                frontend_plugins={"hex-coap": {"kind": "line", "sha256": "abc"}},
                pane_plugins={"A": [{"name": "hex-coap", "options": {}}]},
                plugin_scripts={"hex-coap": "(function(){})()"},
                started_at="2026-01-01T00:00:00+00:00",
                config_path="embed-log.yml",
                job_id="CI-1",
                app_name="demo",
                timestamp_mode="relative",
                first_log_at="2026-01-01T00:00:01.234+00:00",
            )

            info = mgr.build_session_info()
            self.assertEqual(info["id"], "2026-01-01_00-00-00")
            self.assertEqual(info["job_id"], "CI-1")
            self.assertEqual(info["app_name"], "demo")
            self.assertEqual(info["timestamp_mode"], "relative")
            self.assertEqual(info["first_log_at"], "2026-01-01T00:00:01.234+00:00")
            self.assertFalse(info["html_ready"])
            self.assertEqual("pending", info["html_status"])

            mgr.write_manifest(reason="start", exported_html=False)
            manifest = json.loads(mgr.manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["session_id"], "2026-01-01_00-00-00")
            self.assertEqual(manifest["timestamp_mode"], "relative")
            self.assertEqual(manifest["first_log_at"], "2026-01-01T00:00:01.234+00:00")
            self.assertIsNone(manifest["session_html"])
            self.assertEqual("pending", manifest["html_status"])

            mgr.write_manifest(reason="signal", exported_html=True)
            manifest = json.loads(mgr.manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["last_export_reason"], "signal")
            self.assertEqual("pending", manifest["html_status"])
            self.assertTrue(str(mgr.html_path).endswith("session.html"))
            self.assertEqual(manifest["frontend_plugins"], {"hex-coap": {"kind": "line", "sha256": "abc"}})
            self.assertEqual(manifest["pane_plugins"], {"A": [{"name": "hex-coap", "options": {}}]})
            self.assertEqual(manifest["plugin_scripts"], {"hex-coap": "(function(){})()"})


class SessionExporterTests(unittest.TestCase):
    def test_export_success(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            merge_script = td_path / "merge_logs.py"
            merge_script.write_text("# dummy", encoding="utf-8")
            html_out = td_path / "session.html"

            exporter = SessionExporter(
                session_html_path=html_out,
                source_files={"A": str(td_path / "A.log")},
                tabs=[{"label": "Tab", "panes": ["A"]}],
                source_labels={"A": "READER"},
                frontend_plugins={"hex-coap": {"kind": "line", "sha256": "abc"}},
                pane_plugins={"A": [{"name": "hex-coap", "options": {}}]},
                plugin_scripts={"hex-coap": "(function(){})()"},
                timestamp_mode="relative",
                first_log_at="2026-01-01T00:00:01.234+00:00",
                merge_script=merge_script,
                python_executable="python3",
            )

            class Proc:
                returncode = 0
                stderr = ""

            with patch("subprocess.run", return_value=Proc()) as run_mock:
                ok = exporter.export_html("test")

            self.assertTrue(ok)
            args = run_mock.call_args[0][0]
            self.assertIn("--timestamp-mode", args)
            self.assertIn("relative", args)
            self.assertIn("--first-log-at", args)
            self.assertIn("2026-01-01T00:00:01.234+00:00", args)
            self.assertIn("--tab", args)
            self.assertIn("A=READER", args)
            self.assertIn("--output", args)

            self.assertIn("--frontend-plugins-json", args)
            self.assertIn("--pane-plugins-json", args)
            self.assertIn("--plugin-scripts-json", args)
    def test_export_failure_nonzero(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            merge_script = td_path / "merge_logs.py"
            merge_script.write_text("# dummy", encoding="utf-8")

            exporter = SessionExporter(
                session_html_path=td_path / "session.html",
                source_files={"A": str(td_path / "A.log")},
                tabs=[{"label": "Tab", "panes": ["A"]}],
                source_labels={"A": "READER"},
                frontend_plugins={"hex-coap": {"kind": "line", "sha256": "abc"}},
                pane_plugins={"A": [{"name": "hex-coap", "options": {}}]},
                plugin_scripts={"hex-coap": "(function(){})()"},
                timestamp_mode="absolute",
                merge_script=merge_script,
                python_executable="python3",
            )

            class Proc:
                returncode = 1
                stderr = "boom"

            with patch("subprocess.run", return_value=Proc()):
                ok = exporter.export_html("test")

            self.assertFalse(ok)


class SessionSnippetTests(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.session_dir = Path(self.td.name) / "session"
        self.session_dir.mkdir(parents=True, exist_ok=True)
        self.mgr = SessionManager(
            session_id="session",
            session_dir=self.session_dir,
            tabs=[{"label": "T", "panes": ["A", "B"]}],
            source_files={"A": str(self.session_dir / "A.log"), "B": str(self.session_dir / "B.log")},
            source_labels={"A": "READER", "B": "CONTROLLER"},
            frontend_plugins={},
            pane_plugins={},
            plugin_scripts={},
            started_at="2026-01-01T00:00:00+00:00",
            config_path=None,
            job_id=None,
            app_name="test",
        )

    def tearDown(self):
        self.td.cleanup()

    def test_save_snippet_creates_file_and_manifest_entry(self):
        path = self.mgr.save_snippet(
            "line one\nline two\n",
            panes=["A"],
            scope="exact",
            label="alpha",
        )
        self.assertIsNotNone(path)
        self.assertEqual(str(self.session_dir), str(Path(path).parent.parent))

        snippets_dir = self.session_dir / "snippets"
        self.assertTrue(snippets_dir.is_dir())
        files = list(snippets_dir.iterdir())
        self.assertEqual(len(files), 1)
        self.assertEqual(files[0].read_text(encoding="utf-8"), "line one\nline two\n")

        manifest = json.loads((self.session_dir / "manifest.json").read_text(encoding="utf-8"))
        self.assertIn("snippets", manifest)
        self.assertEqual(len(manifest["snippets"]), 1)
        entry = manifest["snippets"][0]
        self.assertEqual(entry["scope"], "exact")
        self.assertEqual(entry["panes"], ["A"])
        self.assertEqual(entry["lines"], 2)
        self.assertIn("created_at", entry)

    def test_save_snippet_empty_text_returns_none(self):
        result = self.mgr.save_snippet("   ", panes=["A"], scope="exact")
        self.assertIsNone(result)
        self.assertFalse((self.session_dir / "snippets").exists())

    def test_save_snippet_enforces_limit(self):
        from backend.session.manager import MAX_SNIPPETS
        for i in range(MAX_SNIPPETS):
            self.mgr.save_snippet(f"line {i}\n", panes=["A"], scope="exact", label=f"s{i}")
        result = self.mgr.save_snippet("overflow\n", panes=["A"], scope="exact")
        self.assertIsNone(result)

        snippets_dir = self.session_dir / "snippets"
        files = list(snippets_dir.iterdir())
        self.assertEqual(len(files), MAX_SNIPPETS)

    def test_save_snippet_context_scope_includes_panes(self):
        path = self.mgr.save_snippet(
            "[A] ctx line\n[B] ctx line\n",
            panes=["A", "B"],
            scope="context-selected",
        )
        self.assertIsNotNone(path)
        manifest = json.loads((self.session_dir / "manifest.json").read_text(encoding="utf-8"))
        entry = manifest["snippets"][0]
        self.assertEqual(entry["scope"], "context-selected")
        self.assertEqual(entry["panes"], ["A", "B"])

if __name__ == "__main__":
    unittest.main()
