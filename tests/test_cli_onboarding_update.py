"""Tests for onboarding, init, update, and enriched doctor output."""

from __future__ import annotations

import argparse
import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from typing import Mapping, Sequence, cast

from backend.cli import main
from backend.cli.config_resolution import ENV_CONFIG_PATH
from backend.cli.sample_config import _list_samples
from backend.cli.update import _run_update_with
from backend.config import load_config


class CliOnboardingTests(unittest.TestCase):
    def setUp(self):
        self._saved_env = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved_env is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved_env

    def test_onboard_json_contains_stable_agent_keys(self):
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = main(["onboard", "--json"])
        self.assertEqual(rc, 0)
        payload = json.loads(buf.getvalue())
        self.assertEqual(
            sorted(payload),
            [
                "active_config",
                "available_samples",
                "commands",
                "docs",
                "install_source",
                "next_steps",
                "recommended_samples",
                "version",
            ],
        )
        self.assertIn("embed-log doctor", {item["command"] for item in payload["commands"]})
        self.assertIn("single_network_single_tab.yml", {item["name"] for item in payload["available_samples"]})
        recommended = payload["recommended_samples"]
        self.assertEqual(
            [item["name"] for item in recommended],
            [
                "single_uart_single_tab.yml",
                "double_uart_single_tab.yml",
                "double_uart_udp_two_tabs.yml",
                "double_uart_network_two_tabs.yml",
                "double_uart_udp_coap_two_tabs.yml",
                "single_file_single_tab.yml",
                "double_uart_file_two_tabs.yml",
            ],
        )


    def test_sample_configs_open_browser_and_verbose(self):
        sample_paths = list(Path("config-samples").glob("*.yml"))
        self.assertTrue(sample_paths)
        for path in sample_paths:
            with self.subTest(path=str(path)):
                cfg = load_config(path)
                self.assertTrue(cfg.server.open_browser)
                self.assertEqual(cfg.server.verbosity, "full")

    def test_onboard_samples_are_canonical_root_configs(self):
        root_samples = sorted(path.name for path in Path("config-samples").glob("*.yml"))
        listed_samples = [path.name for path in _list_samples()]
        self.assertEqual(sorted(listed_samples), root_samples)
        self.assertEqual(listed_samples[0], "single_uart_single_tab.yml")
        self.assertEqual(listed_samples[-1], "reference_full_annotated.yml")
        self.assertFalse(Path("backend/resources/config-samples").exists())

    def test_network_capture_samples_default_to_two_host_udp_filter(self):
        expected = "(host 192.0.2.10 or host 192.0.2.20) and udp"
        sample_paths = [
            Path("config-samples/single_network_single_tab.yml"),
            Path("config-samples/double_uart_network_two_tabs.yml"),
        ]
        for path in sample_paths:
            with self.subTest(path=str(path)):
                cfg = load_config(path)
                capture_sources = [
                    source for source in cfg.sources if source.type == "network_capture"
                ]
                self.assertEqual(len(capture_sources), 1)
                self.assertEqual(capture_sources[0].bpf_filter, expected)

    def test_coap_recommended_sample_uses_hex_coap_plugin(self):
        cfg = load_config("config-samples/double_uart_udp_coap_two_tabs.yml")
        self.assertIn("coap", cfg.frontend_plugins)
        network_tab = next(tab for tab in cfg.tabs if tab.label == "Network")
        plugin_names = [
            plugin.name
            for pane in network_tab.panes
            for plugin in pane.plugins
        ]
        self.assertEqual(plugin_names, ["coap"])

    def test_init_default_writes_double_uart_udp_config_and_next_commands(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "embed-log.yml"
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main(["init", "--output", str(output)])
            self.assertEqual(rc, 0)
            cfg = load_config(output)
            self.assertEqual(
                [source.name for source in cfg.sources],
                ["DUT_UART", "AUX_UART", "PYTEST_UDP"],
            )
            self.assertEqual([source.type for source in cfg.sources], ["uart", "uart", "udp"])
            self.assertTrue(cfg.server.open_browser)
            self.assertEqual(cfg.server.verbosity, "full")
            text = buf.getvalue()
            self.assertIn(f"embed-log doctor --config {output}", text)
            self.assertIn(f"export {ENV_CONFIG_PATH}=\"{output}\"", text)

    def test_init_add_uart_shell_writes_companion_commands_for_uart_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "bench.yml"
            commands_output = Path(tmp) / "bench.commands.yml"
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main(["init", "--add-uart-shell", "--output", str(output)])
            self.assertEqual(rc, 0)
            self.assertTrue(output.is_file())
            text = commands_output.read_text(encoding="utf-8")
            self.assertIn("embed-log run loads this file automatically", text)
            self.assertIn('"DUT_UART":', text)
            self.assertIn('"AUX_UART":', text)
            self.assertNotIn("PYTEST_UDP", text)
            self.assertIn('"help\\r\\n"', text)
            self.assertIn('"status\\r\\n"', text)
            self.assertIn(str(commands_output), buf.getvalue())

    def test_init_add_uart_shell_skips_companion_when_sample_has_no_uart(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "network.yml"
            commands_output = Path(tmp) / "network.commands.yml"
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main(
                    [
                        "init",
                        "--sample",
                        "single_network_single_tab",
                        "--add-uart-shell",
                        "--output",
                        str(output),
                    ]
                )
            self.assertEqual(rc, 0)
            self.assertTrue(output.is_file())
            self.assertFalse(commands_output.exists())
            self.assertIn("No UART sources in selected config", buf.getvalue())

    def test_init_add_uart_shell_refuses_existing_companion_without_force(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "bench.yml"
            commands_output = Path(tmp) / "bench.commands.yml"
            commands_output.write_text("sources: {}\n", encoding="utf-8")
            err = io.StringIO()
            with redirect_stderr(err):
                rc = main(["init", "--add-uart-shell", "--output", str(output)])
            self.assertEqual(rc, 1)
            self.assertFalse(output.exists())
            self.assertIn("Use --force to overwrite", err.getvalue())

    def test_init_add_uart_shell_guides_to_config_flag_when_config_exists(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "bench.yml"
            output.write_text("version: 1\n", encoding="utf-8")
            err = io.StringIO()
            with redirect_stderr(err):
                rc = main(["init", "--add-uart-shell", "--output", str(output)])
            self.assertEqual(rc, 1)
            self.assertIn("To generate only UART shell commands", err.getvalue())
            self.assertIn("embed-log init --config", err.getvalue())

    def test_init_config_add_uart_shell_generates_only_companion_for_existing_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "project.yml"
            original_config = """version: 1
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
"""
            config_path.write_text(original_config, encoding="utf-8")
            commands_output = Path(tmp) / "project.commands.yml"
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main(["init", "--config", str(config_path), "--add-uart-shell"])
            self.assertEqual(rc, 0)
            self.assertEqual(config_path.read_text(encoding="utf-8"), original_config)
            text = commands_output.read_text(encoding="utf-8")
            self.assertIn("from existing config", text)
            self.assertIn('"DUT_UART":', text)
            self.assertIn('"AUX_UART":', text)
            self.assertNotIn("PYTEST_UDP", text)
            self.assertIn(f"embed-log doctor --config {config_path}", buf.getvalue())

    def test_init_config_add_uart_shell_refuses_existing_companion_without_force(self):
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "project.yml"
            config_path.write_text(
                """version: 1
sources:
  - name: DUT_UART
    type: uart
    port: /dev/ttyUSB0
tabs:
  - label: Device
    panes: [DUT_UART]
""",
                encoding="utf-8",
            )
            commands_output = Path(tmp) / "project.commands.yml"
            commands_output.write_text("sources: {}\n", encoding="utf-8")
            err = io.StringIO()
            with redirect_stderr(err):
                rc = main(["init", "--config", str(config_path), "--add-uart-shell"])
            self.assertEqual(rc, 1)
            self.assertEqual(commands_output.read_text(encoding="utf-8"), "sources: {}\n")
            self.assertIn("Use --force to overwrite", err.getvalue())

    def test_init_config_without_uart_shell_flag_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "project.yml"
            config_path.write_text("version: 1\n", encoding="utf-8")
            err = io.StringIO()
            with redirect_stderr(err):
                rc = main(["init", "--config", str(config_path)])
            self.assertEqual(rc, 1)
            self.assertIn("--config is only used with --add-uart-shell", err.getvalue())

    def test_init_accepts_sample_name_without_yml_suffix(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "network.yml"
            with redirect_stdout(io.StringIO()):
                rc = main(["init", "--sample", "single_network_single_tab", "--output", str(output)])
            self.assertEqual(rc, 0)
            cfg = load_config(output)
            self.assertEqual([source.type for source in cfg.sources], ["network_capture"])

    def test_onboard_samples_lists_init_sample_names(self):
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = main(["onboard", "--samples"])
        self.assertEqual(rc, 0)
        out = buf.getvalue()
        self.assertIn("single_network_single_tab", out)
        self.assertIn("single_file_single_tab", out)
        self.assertNotIn("sample-config", out)
        self.assertIn("one packet-capture source in one tab", out)
        self.assertIn("one file-tail source in one tab", out)

    def test_doctor_json_reports_config_details(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg_path = Path(tmp) / "embed-log.yml"
            cfg_path.write_text(
                """version: 1
logs:
  dir: logs/
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
  - name: PYTEST
    type: udp
    port: 6000
tabs:
  - label: Desk
    panes: [DUT, PYTEST]
""",
                encoding="utf-8",
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main(["doctor", "--config", str(cfg_path), "--json"])
            self.assertEqual(rc, 0)
            payload = json.loads(buf.getvalue())
            config = next(section for section in payload["sections"] if section["name"] == "Config")
            checks = {item["check"]: item["status"] for item in config["checks"]}
            self.assertEqual(checks["config exists"], "yes")
            self.assertIn("DUT (uart:/dev/ttyUSB0)", checks["sources"])
            self.assertIn("PYTEST (udp:6000)", checks["sources"])
            self.assertEqual(checks["tabs"], "Desk [DUT, PYTEST]")
            self.assertEqual(checks["panes"], "2 configured")
            self.assertEqual(checks["logs will be written"], "logs/")

    def test_doctor_uses_local_embed_log_yml_when_no_explicit_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg_path = Path(tmp) / "embed-log.yml"
            cfg_path.write_text(
                """version: 1
logs:
  dir: logs/
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
tabs:
  - label: Device
    panes: [DUT]
""",
                encoding="utf-8",
            )
            old_cwd = Path.cwd()
            try:
                os.chdir(tmp)
                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc = main(["doctor", "--json"])
            finally:
                os.chdir(old_cwd)
            self.assertEqual(rc, 0)
            payload = json.loads(buf.getvalue())
            config = next(section for section in payload["sections"] if section["name"] == "Config")
            checks = {item["check"]: item["status"] for item in config["checks"]}
            self.assertIn("local embed-log.yml", checks["effective config"])
            self.assertEqual(checks["config exists"], "yes")


class CliUpdateTests(unittest.TestCase):
    def test_update_latest_passes_stable_installer_environment(self):
        captured: dict[str, object] = {}

        def runner(command: Sequence[str], env: Mapping[str, str]) -> int:
            captured["command"] = list(command)
            captured["env"] = dict(env)
            return 0

        rc = _run_update_with(
            argparse.Namespace(sha=None, allow_rollback=False),
            runner=runner,
        )
        self.assertEqual(rc, 0)
        env = cast(dict[str, str], captured["env"])
        self.assertIsInstance(env, dict)
        self.assertEqual(env["EMBED_LOG_INSTALL_MODE"], "update")
        self.assertEqual(env["EMBED_LOG_REF_TYPE"], "release")
        self.assertEqual(env["EMBED_LOG_REF"], "latest")
        self.assertEqual(env["EMBED_LOG_REPO"], "krezolekcoder/embed-log")
        self.assertEqual(
            env["EMBED_LOG_REPO_URL"],
            "https://github.com/krezolekcoder/embed-log.git",
        )

    def test_update_sha_refuses_commit_older_than_latest_release(self):
        calls: list[str] = []

        def fetcher(url: str) -> Mapping[str, object]:
            calls.append(url)
            if url.endswith("/releases/latest"):
                return {"tag_name": "v9.9.9", "published_at": "2026-06-05T00:00:00Z"}
            if url.endswith("/commits/v9.9.9"):
                return {"commit": {"committer": {"date": "2026-06-01T00:00:00Z"}}}
            return {"commit": {"committer": {"date": "2026-01-01T00:00:00Z"}}}

        def runner(_command: Sequence[str], _env: Mapping[str, str]) -> int:
            self.fail("installer must not run after anti-rollback rejection")

        rc = _run_update_with(
            argparse.Namespace(sha="abcdef1", allow_rollback=False),
            fetcher=fetcher,
            runner=runner,
        )
        self.assertEqual(rc, 1)
        self.assertEqual(len(calls), 3)

    def test_update_sha_allow_rollback_sets_commit_target(self):
        captured: dict[str, object] = {}

        def fetcher(_url: str) -> Mapping[str, object]:
            self.fail("allow-rollback should not need anti-rollback GitHub checks")

        def runner(_command: Sequence[str], env: Mapping[str, str]) -> int:
            captured["env"] = dict(env)
            return 0

        rc = _run_update_with(
            argparse.Namespace(sha="abcdef1", allow_rollback=True),
            fetcher=fetcher,
            runner=runner,
        )
        self.assertEqual(rc, 0)
        env = cast(dict[str, str], captured["env"])
        self.assertIsInstance(env, dict)
        self.assertEqual(env["EMBED_LOG_REF_TYPE"], "commit")
        self.assertEqual(env["EMBED_LOG_REF"], "abcdef1")


if __name__ == "__main__":
    unittest.main()
