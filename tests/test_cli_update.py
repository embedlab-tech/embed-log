import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import patch

from backend import cli


def _ns(**kw):
    return type("Args", (), kw)()


def _pipx_list_json(spec: str) -> str:
    return json.dumps({
        "venvs": {
            "embed-log": {
                "metadata": {
                    "main_package": {
                        "package_or_url": spec,
                    }
                }
            }
        }
    })


class UpdateCommandTests(unittest.TestCase):
    def test_managed_cache_spec_refreshes_and_reinstalls(self):
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            cache_dir = home / ".cache" / "embed-log" / "src"
            spec = str(cache_dir)
            calls = []

            def fake_run(cmd, **kwargs):
                calls.append(cmd)
                if cmd == ["/opt/pipx", "list", "--json"]:
                    return CompletedProcess(cmd, 0, _pipx_list_json(spec), "")
                if cmd[:5] == ["git", "clone", "--depth=1", "-b", "main"]:
                    (cache_dir / "backend").mkdir(parents=True, exist_ok=True)
                    return CompletedProcess(cmd, 0, "", "")
                if cmd == ["git", "-C", str(cache_dir), "rev-parse", "--short", "HEAD"]:
                    return CompletedProcess(cmd, 0, "abc123\n", "")
                if cmd == ["/opt/pipx", "reinstall", "embed-log"]:
                    return CompletedProcess(cmd, 0, "reinstalled\n", "")
                raise AssertionError(f"Unexpected command: {cmd}")

            with patch.object(cli.Path, "home", return_value=home), \
                 patch("backend.cli.shutil.which", side_effect=lambda name: {"pipx": "/opt/pipx", "git": "/usr/bin/git"}.get(name)), \
                 patch("subprocess.run", side_effect=fake_run):
                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc = cli._run_update(_ns(force=False))

            self.assertEqual(rc, 0)
            self.assertIn(["/opt/pipx", "reinstall", "embed-log"], calls)
            self.assertNotIn(["/opt/pipx", "uninstall", "embed-log"], calls)
            self.assertEqual((cache_dir / "backend" / "_version.py").read_text(encoding="utf-8"), (
                "# Auto-generated. Install scripts populate __commit__ before pipx install.\n"
                '__version__ = "0.1.0"\n'
                '__commit__ = "abc123"\n'
            ))
            self.assertIn("embed-log 0.1.0 (abc123)", buf.getvalue())

    def test_stale_local_spec_recovers_via_cache_install(self):
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            cache_dir = home / ".cache" / "embed-log" / "src"
            stale_spec = str(home / "missing" / "embed-log")
            calls = []

            def fake_run(cmd, **kwargs):
                calls.append(cmd)
                if cmd == ["/opt/pipx", "list", "--json"]:
                    return CompletedProcess(cmd, 0, _pipx_list_json(stale_spec), "")
                if cmd[:5] == ["git", "clone", "--depth=1", "-b", "main"]:
                    (cache_dir / "backend").mkdir(parents=True, exist_ok=True)
                    return CompletedProcess(cmd, 0, "", "")
                if cmd == ["git", "-C", str(cache_dir), "rev-parse", "--short", "HEAD"]:
                    return CompletedProcess(cmd, 0, "deadbee\n", "")
                if cmd == ["/opt/pipx", "uninstall", "embed-log"]:
                    return CompletedProcess(cmd, 0, "removed\n", "")
                if cmd == ["/opt/pipx", "install", str(cache_dir)]:
                    return CompletedProcess(cmd, 0, "installed\n", "")
                raise AssertionError(f"Unexpected command: {cmd}")

            with patch.object(cli.Path, "home", return_value=home), \
                 patch("backend.cli.shutil.which", side_effect=lambda name: {"pipx": "/opt/pipx", "git": "/usr/bin/git"}.get(name)), \
                 patch("subprocess.run", side_effect=fake_run):
                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc = cli._run_update(_ns(force=False))

            self.assertEqual(rc, 0)
            self.assertIn(["/opt/pipx", "uninstall", "embed-log"], calls)
            self.assertIn(["/opt/pipx", "install", str(cache_dir)], calls)
            self.assertNotIn(["/opt/pipx", "reinstall", "embed-log"], calls)
            self.assertIn("Install source '" + stale_spec + "' is no longer available.", buf.getvalue())
            self.assertIn("embed-log 0.1.0 (deadbee)", buf.getvalue())

    def test_git_spec_uses_plain_pipx_upgrade(self):
        spec = "git+https://github.com/krezolekcoder/embed-log.git@main"
        calls = []

        def fake_run(cmd, **kwargs):
            calls.append(cmd)
            if cmd == ["/opt/pipx", "list", "--json"]:
                return CompletedProcess(cmd, 0, _pipx_list_json(spec), "")
            if cmd == ["/opt/pipx", "upgrade", "embed-log", "--force"]:
                return CompletedProcess(cmd, 0, "upgraded\n", "")
            raise AssertionError(f"Unexpected command: {cmd}")

        with patch("backend.cli.shutil.which", side_effect=lambda name: {"pipx": "/opt/pipx"}.get(name)), \
             patch("subprocess.run", side_effect=fake_run):
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = cli._run_update(_ns(force=True))

        self.assertEqual(rc, 0)
        self.assertEqual(calls, [
            ["/opt/pipx", "list", "--json"],
            ["/opt/pipx", "upgrade", "embed-log", "--force"],
        ])
        self.assertIn("Running: /opt/pipx upgrade embed-log --force", buf.getvalue())


if __name__ == "__main__":
    unittest.main()
