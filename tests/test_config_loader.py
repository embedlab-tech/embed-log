import tempfile
import unittest
from pathlib import Path

from backend.config import AppConfig, ConfigError, load_config



class ConfigLoaderTests(unittest.TestCase):
    def test_load_valid_config_with_server_fields(self):
        cfg_text = """
version: 1
server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: demo
  open_browser: true
  verbosity: events
  queue_size: 32768
  timestamp_mode: relative
  job_id: CI-42
logs:
  dir: logs/
baudrate: 115200
sources:
  - name: UART_A
    label: READER
    type: uart
    port: /dev/ttyUSB0
    inject_port: 5001
    forward_ports: [7001, 7002]
  - name: UDP_A
    label: CONTROLLER
    type: udp
    port: 6000
tabs:
  - label: Devices
    panes: [UART_A, UDP_A]
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            cfg = load_config(p)

        self.assertIsInstance(cfg, AppConfig)
        self.assertEqual(cfg.server.host, "127.0.0.1")
        self.assertEqual(cfg.server.ws_port, 8080)
        self.assertEqual(cfg.server.app_name, "demo")
        self.assertTrue(cfg.server.open_browser)
        self.assertEqual(cfg.server.verbosity, "events")
        self.assertEqual(cfg.server.timestamp_mode, "relative")
        self.assertEqual(cfg.server.job_id, "CI-42")
        self.assertEqual(cfg.server.queue_size, 32768)
        self.assertEqual(cfg.logs.dir, "logs/")
        self.assertEqual(len(cfg.sources), 2)
        self.assertEqual(len(cfg.injects), 1)
        self.assertEqual(len(cfg.forwards), 2)
        self.assertEqual(len(cfg.tabs), 1)
        self.assertEqual(cfg.source_labels, {"UART_A": "READER", "UDP_A": "CONTROLLER"})
        self.assertEqual(cfg.sources[0].parser.type, "text")
        self.assertEqual(cfg.sources[1].parser.type, "text")

    def test_invalid_verbosity_fails(self):
        cfg_text = """
version: 1
server:
  verbosity: noisy
sources:
  - name: A
    type: udp
    port: 6000
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)

    def test_invalid_timestamp_mode_fails(self):
        cfg_text = """
version: 1
server:
  timestamp_mode: monotonic
sources:
  - name: A
    type: udp
    port: 6000
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)

    def test_duplicate_source_name_fails(self):
        cfg_text = """
version: 1
sources:
  - name: A
    type: udp
    port: 6000
  - name: A
    type: udp
    port: 6001
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)

    def test_explicit_text_parser_is_accepted(self):
        cfg_text = """
version: 1
sources:
  - name: A
    type: uart
    port: /dev/ttyUSB0
    parser:
      type: text
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            cfg = load_config(p)

        self.assertEqual(cfg.sources[0].parser.type, "text")

    def test_unsupported_parser_type_fails(self):
        cfg_text = """
version: 1
sources:
  - name: A
    type: uart
    port: /dev/ttyUSB0
    parser:
      type: command
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)

    def test_cbor_datagram_parser_accepted_on_udp(self):
        cfg_text = """
version: 1
sources:
  - name: A
    type: udp
    port: 6000
    parser:
      type: cbor-datagram
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            cfg = load_config(p)

        self.assertEqual(cfg.sources[0].parser.type, "cbor-datagram")

    def test_cbor_datagram_parser_rejected_on_uart(self):
        """cbor-datagram is only valid for UDP (datagram-oriented)."""
        cfg_text = """
version: 1
sources:
  - name: A
    type: uart
    port: /dev/ttyUSB0
    parser:
      type: cbor-datagram
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)

    def test_parser_extra_field_fails(self):
        """Extra fields in parser config are rejected."""
        cfg_text = """
version: 1
sources:
  - name: A
    type: udp
    port: 6000
    parser:
      type: text
      format: extended
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)

if __name__ == "__main__":
    unittest.main()
