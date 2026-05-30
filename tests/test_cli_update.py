import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import patch

from backend.cli import update as cli
from backend import _install_source as install_source


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


class _Response:
    def __init__(self, data: bytes):
        self._data = data

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
        return self._data


class UpdateCommandTests(unittest.TestCase):
    def test_local_source_runs_local_installer(self):
        with tempfile.TemporaryDirectory() as td:
            repo = Path(td) / "repo"
            repo.mkdir()
            (repo / "install.sh").write_text("#!/usr/bin/env bash\n", encoding="utf-8")
            (repo / "install.ps1").write_text("", encoding="utf-8")

            calls = []

            def fake_run(cmd, **kwargs):
                calls.append((cmd, kwargs))
                if cmd == ["/opt/pipx", "list", "--json"]:
                    return CompletedProcess(cmd, 0, _pipx_list_json(str(repo)), "")
                if cmd == ["/bin/bash", str(repo / "install.sh")]:
                    return CompletedProcess(cmd, 0, "", "")
                raise AssertionError(f"Unexpected command: {cmd}")

            with patch.object(install_source, "__source_kind__", "local"), \
                 patch.object(install_source, "__local_path__", str(repo)), \
                 patch.object(install_source, "__repo__", "krezolekcoder/embed-log"), \
                 patch.object(install_source, "__repo_url__", "https://github.com/krezolekcoder/embed-log.git"), \
                 patch.object(install_source, "__ref_type__", "branch"), \
                 patch.object(install_source, "__ref__", "main"), \
                 patch("backend.cli.update.shutil.which", side_effect=lambda name: {"pipx": "/opt/pipx", "bash": "/bin/bash"}.get(name)), \
                 patch("subprocess.run", side_effect=fake_run):
                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc = cli._run_update(_ns(force=False, branch=None, tag=None, ref=None, release=False))

            self.assertEqual(rc, 0)
            self.assertIn("Running installer from local source:", buf.getvalue())
            self.assertEqual(calls[1][0], ["/bin/bash", str(repo / "install.sh")])

    def test_remote_override_downloads_and_runs_installer(self):
        calls = []
        seen_env = {}

        def fake_run(cmd, **kwargs):
            calls.append((cmd, kwargs))
            if cmd == ["/opt/pipx", "list", "--json"]:
                return CompletedProcess(cmd, 0, _pipx_list_json("git+https://github.com/krezolekcoder/embed-log.git@main"), "")
            if cmd[0] == "/bin/bash":
                seen_env.update(kwargs["env"])
                return CompletedProcess(cmd, 0, "", "")
            raise AssertionError(f"Unexpected command: {cmd}")

        with patch.object(install_source, "__source_kind__", "git"), \
             patch.object(install_source, "__local_path__", ""), \
             patch.object(install_source, "__repo__", "krezolekcoder/embed-log"), \
             patch.object(install_source, "__repo_url__", "https://github.com/krezolekcoder/embed-log.git"), \
             patch.object(install_source, "__ref_type__", "branch"), \
             patch.object(install_source, "__ref__", "main"), \
             patch("backend.cli.update.shutil.which", side_effect=lambda name: {"pipx": "/opt/pipx", "bash": "/bin/bash"}.get(name)), \
             patch("urllib.request.urlopen", return_value=_Response(b"#!/usr/bin/env bash\n")), \
             patch("subprocess.run", side_effect=fake_run):
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = cli._run_update(_ns(force=False, branch="feature/demo", tag=None, ref=None, release=False))

        self.assertEqual(rc, 0)
        self.assertIn("Running installer from krezolekcoder/embed-log@branch:feature/demo", buf.getvalue())
        self.assertEqual(seen_env["EMBED_LOG_REF_TYPE"], "branch")
        self.assertEqual(seen_env["EMBED_LOG_REF"], "feature/demo")
        self.assertEqual(seen_env["EMBED_LOG_REPO"], "krezolekcoder/embed-log")

    def test_missing_local_source_requires_explicit_remote_ref(self):
        missing = "/tmp/no-such-embed-log-source"
        with patch.object(install_source, "__source_kind__", "local"), \
             patch.object(install_source, "__local_path__", missing), \
             patch.object(install_source, "__repo__", "krezolekcoder/embed-log"), \
             patch.object(install_source, "__repo_url__", "https://github.com/krezolekcoder/embed-log.git"), \
             patch.object(install_source, "__ref_type__", "branch"), \
             patch.object(install_source, "__ref__", "main"), \
             patch("backend.cli.update.shutil.which", side_effect=lambda name: {"pipx": "/opt/pipx", "bash": "/bin/bash"}.get(name)), \
             patch("subprocess.run", return_value=CompletedProcess(["/opt/pipx", "list", "--json"], 0, _pipx_list_json(missing), "")):
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = cli._run_update(_ns(force=False, branch=None, tag=None, ref=None, release=False))

        self.assertEqual(rc, 1)
        self.assertIn("Local install source is unavailable", buf.getvalue())
        self.assertIn("embed-log update --release", buf.getvalue())

    def test_release_override_resolves_latest_tag_before_download(self):
        calls = []
        seen_env = {}
        responses = [
            _Response(b'{"tag_name":"v1.0.0"}'),
            _Response(b"#!/usr/bin/env bash\n"),
        ]

        def fake_run(cmd, **kwargs):
            calls.append((cmd, kwargs))
            if cmd == ["/opt/pipx", "list", "--json"]:
                return CompletedProcess(cmd, 0, _pipx_list_json("git+https://github.com/krezolekcoder/embed-log.git@main"), "")
            if cmd[0] == "/bin/bash":
                seen_env.update(kwargs["env"])
                return CompletedProcess(cmd, 0, "", "")
            raise AssertionError(f"Unexpected command: {cmd}")

        def fake_urlopen(url, timeout=30):
            self.assertIn(url, [
                "https://api.github.com/repos/krezolekcoder/embed-log/releases/latest",
                "https://raw.githubusercontent.com/krezolekcoder/embed-log/v1.0.0/install.sh",
            ])
            return responses.pop(0)

        with patch.object(install_source, "__source_kind__", "git"), \
             patch.object(install_source, "__local_path__", ""), \
             patch.object(install_source, "__repo__", "krezolekcoder/embed-log"), \
             patch.object(install_source, "__repo_url__", "https://github.com/krezolekcoder/embed-log.git"), \
             patch.object(install_source, "__ref_type__", "branch"), \
             patch.object(install_source, "__ref__", "main"), \
             patch("backend.cli.update.shutil.which", side_effect=lambda name: {"pipx": "/opt/pipx", "bash": "/bin/bash"}.get(name)), \
             patch("urllib.request.urlopen", side_effect=fake_urlopen), \
             patch("subprocess.run", side_effect=fake_run):
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = cli._run_update(_ns(force=False, branch=None, tag=None, ref=None, release=True))

        self.assertEqual(rc, 0)
        self.assertIn("Running installer from krezolekcoder/embed-log@release:v1.0.0", buf.getvalue())
        self.assertEqual(seen_env["EMBED_LOG_REF_TYPE"], "release")
        self.assertEqual(seen_env["EMBED_LOG_REF"], "latest")

if __name__ == "__main__":
    unittest.main()
