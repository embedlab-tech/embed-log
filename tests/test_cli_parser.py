"""Tests for backend.cli_parser — argument parser structure and defaults."""

from __future__ import annotations

import unittest

from backend.cli.parser import build_parser


class ParserStructureTests(unittest.TestCase):
    """Verify that build_parser produces a valid parser with all subcommands."""

    @classmethod
    def setUpClass(cls):
        cls.parser = build_parser()

    def test_parser_accepts_no_args(self):
        """No args should parse without error (command=None)."""
        args = self.parser.parse_args([])
        self.assertIsNone(args.command)

    def test_subcommands_present(self):
        """All expected subcommands must be registered."""
        expected = {
            "run", "demo", "sessions", "merge", "parse",
            "tail-file", "version", "ports", "sample-config", "skill",
        }
        # subparsers are stored in the _subparsers action
        subparser_actions = [
            action for action in self.parser._subparsers._group_actions
            if hasattr(action, "_parser_class")
        ]
        # The choices dict contains all subcommand names (including aliases)
        choices = {}
        for action in subparser_actions:
            choices.update(action._name_parser_map)
        # Check that all expected commands are present
        for cmd in expected:
            self.assertIn(cmd, choices, f"subcommand {cmd!r} not found in parser")

class RunSubcommandTests(unittest.TestCase):
    """Test 'run' subcommand argument parsing."""

    @classmethod
    def setUpClass(cls):
        cls.parser = build_parser()

    def test_run_defaults(self):
        args = self.parser.parse_args(["run"])
        self.assertIsNone(args.config)
        self.assertEqual(args.sources, [])
        self.assertEqual(args.injects, [])
        self.assertEqual(args.forwards, [])
        self.assertEqual(args.tabs, [])
        self.assertIsNone(args.baudrate)
        self.assertIsNone(args.log_dir)
        self.assertIsNone(args.host)
        self.assertIsNone(args.ws_port)
        self.assertIsNone(args.ws_ui)
        self.assertIsNone(args.app_name)
        self.assertIsNone(args.open_browser)
        self.assertIsNone(args.timestamp_mode)
        self.assertIsNone(args.verbosity)
        self.assertIsNone(args.verbose)
        self.assertIsNone(args.verbose_full)
        self.assertIsNone(args.job_id)

    def test_run_config(self):
        args = self.parser.parse_args(["run", "--config", "my.yml"])
        self.assertEqual(args.config, "my.yml")

    def test_run_config_short(self):
        args = self.parser.parse_args(["run", "-c", "my.yml"])
        self.assertEqual(args.config, "my.yml")

    def test_run_source(self):
        args = self.parser.parse_args([
            "run", "--source", "SENSOR_A", "udp:6000"
        ])
        self.assertEqual(args.sources, [["SENSOR_A", "udp:6000"]])

    def test_run_multiple_sources(self):
        args = self.parser.parse_args([
            "run",
            "--source", "A", "udp:6000",
            "--source", "B", "uart:/dev/ttyUSB0",
        ])
        self.assertEqual(len(args.sources), 2)

    def test_run_inject(self):
        args = self.parser.parse_args([
            "run", "--inject", "SENSOR_A", "5001"
        ])
        self.assertEqual(args.injects, [["SENSOR_A", "5001"]])

    def test_run_forward(self):
        args = self.parser.parse_args([
            "run", "--forward", "SENSOR_A", "7001"
        ])
        self.assertEqual(args.forwards, [["SENSOR_A", "7001"]])

    def test_run_tab(self):
        args = self.parser.parse_args([
            "run", "--tab", "Devices", "SENSOR_A", "SENSOR_B"
        ])
        self.assertEqual(args.tabs, [["Devices", "SENSOR_A", "SENSOR_B"]])

    def test_run_baudrate(self):
        args = self.parser.parse_args(["run", "--baudrate", "9600"])
        self.assertEqual(args.baudrate, 9600)

    def test_run_open_browser(self):
        args = self.parser.parse_args(["run", "--open-browser"])
        self.assertTrue(args.open_browser)

    def test_run_no_open_browser(self):
        args = self.parser.parse_args(["run", "--no-open-browser"])
        self.assertFalse(args.open_browser)

    def test_run_timestamp_mode(self):
        args = self.parser.parse_args(["run", "--timestamp-mode", "relative"])
        self.assertEqual(args.timestamp_mode, "relative")

    def test_run_timestamp_mode_invalid(self):
        with self.assertRaises(SystemExit):
            self.parser.parse_args(["run", "--timestamp-mode", "bogus"])

    def test_run_verbosity(self):
        args = self.parser.parse_args(["run", "--verbosity", "quiet"])
        self.assertEqual(args.verbosity, "quiet")

    def test_run_verbosity_invalid(self):
        with self.assertRaises(SystemExit):
            self.parser.parse_args(["run", "--verbosity", "loud"])

    def test_run_verbose_short(self):
        args = self.parser.parse_args(["run", "-v"])
        self.assertTrue(args.verbose)

    def test_run_job_id(self):
        args = self.parser.parse_args(["run", "--job-id", "GH-12345"])
        self.assertEqual(args.job_id, "GH-12345")


class MergeSubcommandTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.parser = build_parser()

    def test_merge_requires_tab(self):
        with self.assertRaises(SystemExit):
            self.parser.parse_args(["merge"])

    def test_merge_tab(self):
        args = self.parser.parse_args([
            "merge", "--tab", "MyTab", "SENSOR_A", "sensor.log"
        ])
        self.assertEqual(args.tab, [["MyTab", "SENSOR_A", "sensor.log"]])

    def test_merge_output_default(self):
        args = self.parser.parse_args([
            "merge", "--tab", "T", "S", "f.log"
        ])
        self.assertEqual(args.output, "merged.html")


class ParseSubcommandTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.parser = build_parser()

    def test_parse_requires_html(self):
        with self.assertRaises(SystemExit):
            self.parser.parse_args(["parse"])

    def test_parse_html(self):
        args = self.parser.parse_args(["parse", "session.html"])
        self.assertEqual(args.html, "session.html")
        self.assertIsNone(args.output)


class TailFileSubcommandTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.parser = build_parser()

    def test_tail_file_defaults(self):
        args = self.parser.parse_args([
            "tail-file", "app.log", "127.0.0.1:6000"
        ])
        self.assertEqual(args.path, "app.log")
        self.assertEqual(args.target, ("127.0.0.1", 6000))
        self.assertFalse(args.from_start)
        self.assertAlmostEqual(args.poll_interval, 0.2)
        self.assertEqual(args.encoding, "utf-8")

    def test_tail_file_from_start(self):
        args = self.parser.parse_args([
            "tail-file", "app.log", "127.0.0.1:6000", "--from-start"
        ])
        self.assertTrue(args.from_start)


class VersionSubcommandTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.parser = build_parser()

    def test_version_defaults(self):
        args = self.parser.parse_args(["version"])
        self.assertIsNone(args.config)
        self.assertFalse(args.json)

    def test_version_json(self):
        args = self.parser.parse_args(["version", "--json"])
        self.assertTrue(args.json)


class PortsSubcommandTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.parser = build_parser()

    def test_ports_defaults(self):
        args = self.parser.parse_args(["ports"])
        self.assertFalse(args.json)

    def test_ports_json(self):
        args = self.parser.parse_args(["ports", "--json"])
        self.assertTrue(args.json)


if __name__ == "__main__":
    unittest.main()
