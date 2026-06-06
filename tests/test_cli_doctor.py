"""Tests for the ``embed-log doctor`` diagnostic command."""

from __future__ import annotations

import argparse
import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

from backend.cli import main
from backend.cli.config_resolution import ENV_CONFIG_PATH
from backend.cli.diagnostics import _collect_doctor_sections, _run_doctor


def _ns(**kw) -> argparse.Namespace:
    return argparse.Namespace(**kw)


class DoctorSectionTests(unittest.TestCase):
    """Section assembly is pure and easy to reason about in isolation."""

    def setUp(self):
        self._saved = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved

    def test_sections_have_expected_names(self):
        sections = _collect_doctor_sections(_ns(config=None))
        self.assertEqual(
            [name for name, _ in sections],
            ["Environment", "Config", "Install", "Runtime"],
        )

    def test_env_var_set_and_file_present(self):
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cfg_file:
            cfg_file.write("x: 1\n")
            env_path = Path(cfg_file.name)
        try:
            os.environ[ENV_CONFIG_PATH] = str(env_path)
            sections = _collect_doctor_sections(_ns(config=None))
            env_section = dict(sections[0][1])
            self.assertIn(ENV_CONFIG_PATH, env_section)
            self.assertIn("set, file present", env_section[ENV_CONFIG_PATH])
            self.assertIn(str(env_path), env_section[ENV_CONFIG_PATH])
        finally:
            env_path.unlink()

    def test_env_var_set_but_file_missing(self):
        os.environ[ENV_CONFIG_PATH] = "/no/such/env.yml"
        sections = _collect_doctor_sections(_ns(config=None))
        env_section = dict(sections[0][1])
        self.assertIn("MISSING", env_section[ENV_CONFIG_PATH])

    def test_env_var_not_set_reports_explicitly(self):
        sections = _collect_doctor_sections(_ns(config=None))
        env_section = dict(sections[0][1])
        self.assertEqual(env_section[ENV_CONFIG_PATH], "(not set)")

    def test_default_config_presence_indicator(self):
        sections = _collect_doctor_sections(_ns(config=None))
        config_section = dict(sections[1][1])
        # cwd is the test runner's directory; we only care that the label is
        # present and clearly indicates presence/absence.
        self.assertIn("default config", config_section)
        label = config_section["default config"]
        self.assertTrue(
            label.endswith("(present)") or label.endswith("(not present)"),
            msg=label,
        )

    def test_effective_config_from_explicit(self):
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cfg_file:
            cfg_file.write("x: 1\n")
            explicit = Path(cfg_file.name)
        try:
            os.environ[ENV_CONFIG_PATH] = "/no/such/env.yml"
            sections = _collect_doctor_sections(_ns(config=str(explicit)))
            config_section = dict(sections[1][1])
            self.assertIn("--config", config_section["effective config"])
            self.assertIn(str(explicit), config_section["effective config"])
        finally:
            explicit.unlink()

    def test_effective_config_from_env(self):
        with tempfile.NamedTemporaryFile(
            suffix=".yml", mode="w", delete=False
        ) as cfg_file:
            cfg_file.write("x: 1\n")
            env_path = Path(cfg_file.name)
        try:
            os.environ[ENV_CONFIG_PATH] = str(env_path)
            sections = _collect_doctor_sections(_ns(config=None))
            config_section = dict(sections[1][1])
            self.assertIn(ENV_CONFIG_PATH, config_section["effective config"])
            self.assertIn(str(env_path), config_section["effective config"])
        finally:
            env_path.unlink()

    def test_effective_config_none_when_unset(self):
        old_cwd = Path.cwd()
        with tempfile.TemporaryDirectory() as tmp:
            try:
                os.chdir(tmp)
                sections = _collect_doctor_sections(_ns(config=None))
            finally:
                os.chdir(old_cwd)
        config_section = dict(sections[1][1])
        self.assertIn("none", config_section["effective config"])
        self.assertIn("inline flags", config_section["effective config"])


class DoctorCommandTests(unittest.TestCase):
    """End-to-end rendering of the doctor command."""

    def setUp(self):
        self._saved = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved

    def test_human_output_has_all_sections(self):
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_doctor(_ns(config=None, json=False))
        self.assertEqual(rc, 0)
        out = buf.getvalue()
        self.assertIn("embed-log doctor", out)
        for name in ("Environment:", "Config:", "Install:", "Runtime:"):
            self.assertIn(name, out)
        self.assertIn("All checks passed.", out)

    def test_env_var_visible_in_human_output(self):
        os.environ[ENV_CONFIG_PATH] = "/no/such/env.yml"
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_doctor(_ns(config=None, json=False))
        self.assertEqual(rc, 1)
        out = buf.getvalue()
        self.assertIn(ENV_CONFIG_PATH, out)
        self.assertIn("MISSING", out)
        self.assertIn("Some checks failed.", out)

    def test_json_output_shape(self):
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_doctor(_ns(config=None, json=True))
        self.assertEqual(rc, 0)
        payload = json.loads(buf.getvalue())
        self.assertIn("ok", payload)
        self.assertIn("sections", payload)
        names = [s["name"] for s in payload["sections"]]
        self.assertEqual(
            names, ["Environment", "Config", "Install", "Runtime"]
        )
        for section in payload["sections"]:
            self.assertIn("checks", section)
            for check in section["checks"]:
                self.assertIn("check", check)
                self.assertIn("status", check)

    def test_json_reports_uart_command_file_counts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config_path = root / "bench.yml"
            config_path.write_text(
                """version: 1
logs:
  dir: logs/
sources:
  - name: DUT_UART
    type: uart
    port: /dev/ttyUSB0
  - name: AUX_UART
    type: uart
    port: /dev/ttyUSB1
  - name: PYTEST_UDP
    type: udp
    port: 6000
tabs:
  - label: Device
    panes: [DUT_UART, AUX_UART]
  - label: CI
    panes: [PYTEST_UDP]
""",
                encoding="utf-8",
            )
            (root / "bench.commands.yml").write_text(
                """sources:
  DUT_UART:
    - "help\\r\\n"
    - "status\\r\\n"
  AUX_UART:
    - "version\\r\\n"
  PYTEST_UDP:
    - "ignored for TX UI"
  UNKNOWN:
    - "ignored"
""",
                encoding="utf-8",
            )

            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = _run_doctor(_ns(config=str(config_path), json=True))

        self.assertEqual(rc, 0)
        payload = json.loads(buf.getvalue())
        config_section = next(
            section for section in payload["sections"] if section["name"] == "Config"
        )
        checks = {item["check"]: item["status"] for item in config_section["checks"]}
        self.assertEqual(checks["UART command file"], str(config_path.with_name("bench.commands.yml")))
        self.assertIn("DUT_UART: 2", checks["UART commands"])
        self.assertIn("AUX_UART: 1", checks["UART commands"])
        self.assertNotIn("PYTEST_UDP", checks["UART commands"])
        self.assertEqual(checks["UART command sources ignored"], "PYTEST_UDP, UNKNOWN")

    def test_json_reports_failure_for_missing_env_config(self):
        os.environ[ENV_CONFIG_PATH] = "/no/such/env.yml"
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = _run_doctor(_ns(config=None, json=True))
        self.assertEqual(rc, 1)
        payload = json.loads(buf.getvalue())
        self.assertFalse(payload["ok"])
        env_section = next(
            s for s in payload["sections"] if s["name"] == "Environment"
        )
        env_pairs = {c["check"]: c["status"] for c in env_section["checks"]}
        self.assertIn(ENV_CONFIG_PATH, env_pairs)
        self.assertIn("MISSING", env_pairs[ENV_CONFIG_PATH])


class DoctorDispatchTests(unittest.TestCase):
    """``embed-log doctor`` is reachable through the top-level dispatcher."""

    def setUp(self):
        self._saved = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved

    def test_doctor_dispatches_through_main(self):
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = main(["doctor"])
        self.assertEqual(rc, 0)
        self.assertIn("embed-log doctor", buf.getvalue())

    def test_doctor_json_dispatches_through_main(self):
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = main(["doctor", "--json"])
        self.assertEqual(rc, 0)
        payload = json.loads(buf.getvalue())
        self.assertIn("sections", payload)

    def test_doctor_help_lists_config_flag(self):
        buf = io.StringIO()
        with patch("sys.stdout", buf):
            with self.assertRaises(SystemExit):
                main(["doctor", "--help"])
        self.assertIn("--config", buf.getvalue())
        self.assertIn("--json", buf.getvalue())


if __name__ == "__main__":
    unittest.main()
