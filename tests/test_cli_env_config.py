"""End-to-end tests for EMBED_LOG_CONFIG_YML_PATH precedence and failures."""

from __future__ import annotations


import argparse
import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

from backend.cli import main
from backend.cli.config_resolution import ENV_CONFIG_PATH
from backend.cli.diagnostics import _run_version
from backend.cli.run import _run_run


def _ns(**kw) -> argparse.Namespace:
    return argparse.Namespace(**kw)

def _minimal_args(config=None):
    """Build the args namespace `_run_run` expects (no sources, no config)."""
    return _ns(
        config=config,
        sources=[],
        injects=[],
        forwards=[],
        tabs=[],
        baudrate=None,
        log_dir=None,
        host=None,
        ws_port=None,
        ws_ui=None,
        app_name=None,
        open_browser=None,
        timestamp_mode=None,
        default_light_theme=None,
        default_dark_theme=None,
        verbosity="quiet",
        verbose=False,
        verbose_full=False,
        job_id=None,
    )


class EnvConfigVarInRunTests(unittest.TestCase):
    def setUp(self):
        self._saved = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved

    def test_env_var_pointing_to_missing_file_fails_loudly(self):
        os.environ[ENV_CONFIG_PATH] = "/nonexistent/embed-log.yml"
        buf = io.StringIO()
        with redirect_stderr(buf):
            rc = _run_run(_minimal_args())
        self.assertEqual(rc, 1)
        out = buf.getvalue()
        self.assertIn(ENV_CONFIG_PATH, out)
        self.assertIn("/nonexistent/embed-log.yml", out)
        self.assertIn("missing or unreadable", out)

    def test_explicit_config_pointing_to_missing_file_fails_loudly(self):
        buf = io.StringIO()
        with redirect_stderr(buf):
            rc = _run_run(_minimal_args(config="/nonexistent/cli.yml"))
        self.assertEqual(rc, 1)
        out = buf.getvalue()
        self.assertIn("--config", out)
        self.assertIn("/nonexistent/cli.yml", out)
        self.assertIn("missing or unreadable", out)

    def test_env_var_does_not_shadow_explicit_config(self):
        """When --config is given, env var must not be consulted at all."""
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cli_cfg:
            cli_cfg.write(
                "app_name: cli\n"
                "sources:\n"
                "  - name: SENSOR_A\n"
                "    type: udp\n"
                "    port: 0\n"
            )
            cli_path = Path(cli_cfg.name)
        try:
            os.environ[ENV_CONFIG_PATH] = "/definitely/does/not/exist.yml"
            buf = io.StringIO()
            with patch("backend.cli.run.run_app") as mock_run_app:
                mock_run_app.return_value = 0
                with redirect_stderr(buf):
                    rc = _run_run(_minimal_args(config=str(cli_path)))
            # The env var must not be consulted, so the "missing or unreadable"
            # error for the env path must not appear.
            self.assertNotIn(ENV_CONFIG_PATH, buf.getvalue())
            self.assertNotIn("/definitely/does/not/exist.yml", buf.getvalue())
        finally:
            cli_path.unlink()

    def test_no_env_no_config_hits_no_sources_error(self):
        """The no-sources error message must mention the env var as a fallback."""
        buf = io.StringIO()
        with redirect_stderr(buf):
            rc = _run_run(_minimal_args())
        self.assertEqual(rc, 1)
        out = buf.getvalue()
        self.assertIn(ENV_CONFIG_PATH, out)
        self.assertIn("--config", out)

    def test_manifest_runtime_config_path_receives_resolved_env(self):
        """`_run_run` must propagate the env-var path into the runtime."""
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cfg_file:
            cfg_file.write(
                "app_name: from-env\n"
                "sources:\n"
                "  - name: SENSOR_A\n"
                "    type: udp\n"
                "    port: 0\n"
            )
            env_path = Path(cfg_file.name)
        try:
            os.environ[ENV_CONFIG_PATH] = str(env_path)
            captured: dict = {}

            with patch("backend.cli.run.run_app") as mock_run_app:
                mock_run_app.return_value = 0

                def _capture(**kwargs):
                    captured.update(kwargs)
                    return 0

                mock_run_app.side_effect = _capture
                rc = _run_run(_minimal_args())

            self.assertEqual(rc, 0)
            self.assertEqual(captured.get("config_path"), str(env_path))
        finally:
            env_path.unlink()


class EnvConfigVarInVersionTests(unittest.TestCase):
    def setUp(self):
        self._saved = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved

    def test_version_uses_env_config(self):
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cfg_file:
            cfg_file.write("app_name: from-env\nsources: []\n")
            env_path = Path(cfg_file.name)
        try:
            os.environ[ENV_CONFIG_PATH] = str(env_path)
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = _run_version(_ns(config=None, json=True))
            self.assertEqual(rc, 0)
            payload = json.loads(buf.getvalue())
            checks = {c["check"]: c["status"] for c in payload["checks"]}
            self.assertEqual(checks.get("config"), str(env_path))
        finally:
            env_path.unlink()

    def test_version_reports_env_config_missing(self):
        os.environ[ENV_CONFIG_PATH] = "/no/such/env.yml"
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_version(_ns(config=None, json=True))
        self.assertEqual(rc, 1)
        payload = json.loads(buf.getvalue())
        checks = {c["check"]: c["status"] for c in payload["checks"]}
        self.assertIn("NOT_FOUND", checks.get("config", ""))
        self.assertIn(ENV_CONFIG_PATH, checks.get("config", ""))

    def test_explicit_config_overrides_env(self):
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cli_cfg:
            cli_cfg.write("app_name: cli\nsources: []\n")
            cli_path = Path(cli_cfg.name)
        os.environ[ENV_CONFIG_PATH] = "/no/such/env.yml"
        try:
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = _run_version(_ns(config=str(cli_path), json=True))
            self.assertEqual(rc, 0)
            payload = json.loads(buf.getvalue())
            checks = {c["check"]: c["status"] for c in payload["checks"]}
            self.assertEqual(checks.get("config"), str(cli_path))
        finally:
            cli_path.unlink()


class EnvConfigVarInMainDispatchTests(unittest.TestCase):
    def setUp(self):
        self._saved = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved

    def test_no_args_with_env_config_suggests_run(self):
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cfg_file:
            cfg_file.write("app_name: x\nsources: []\n")
            env_path = Path(cfg_file.name)
        try:
            os.environ[ENV_CONFIG_PATH] = str(env_path)
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main([])
            self.assertEqual(rc, 0)
            out = buf.getvalue()
            self.assertIn(ENV_CONFIG_PATH, out)
            self.assertIn("embed-log run", out)
            self.assertIn(str(env_path), out)
        finally:
            env_path.unlink()


if __name__ == "__main__":
    unittest.main()
