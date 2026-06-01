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
        self.assertEqual(cfg.tabs[0].panes[0].source, "UART_A")
        self.assertEqual(cfg.tabs[0].panes[1].source, "UDP_A")
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
    def test_frontend_plugin_config_is_loaded(self):
        cfg_text = """
version: 1
frontend_plugins:
  hex-coap:
    builtin: hex-coap
sources:
  - name: UDP_A
    type: udp
    port: 6000
tabs:
  - label: Devices
    panes:
      - source: UDP_A
        plugins:
          - name: hex-coap
            options:
              protocol: coap
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            cfg = load_config(p)

        self.assertIn("hex-coap", cfg.frontend_plugins)
        pane = cfg.tabs[0].panes[0]
        self.assertEqual(pane.source, "UDP_A")
        self.assertEqual(len(pane.plugins), 1)
        self.assertEqual(pane.plugins[0].name, "hex-coap")
        self.assertEqual(pane.plugins[0].options, {"protocol": "coap"})

    def test_unknown_frontend_plugin_reference_fails(self):
        cfg_text = """
version: 1
sources:
  - name: UDP_A
    type: udp
    port: 6000
tabs:
  - label: Devices
    panes:
      - source: UDP_A
        plugins:
          - missing-plugin
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)

    def test_conflicting_plugin_sets_for_same_source_fail(self):
        cfg_text = """
version: 1
frontend_plugins:
  hex-coap:
    builtin: hex-coap
sources:
  - name: UDP_A
    type: udp
    port: 6000
tabs:
  - label: Devices
    panes:
      - source: UDP_A
        plugins: [hex-coap]
  - label: Alt
    panes:
      - source: UDP_A
""".strip()
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "cfg.yml"
            p.write_text(cfg_text, encoding="utf-8")
            with self.assertRaises(ConfigError):
                load_config(p)
    def test_bundled_demo_config_enables_hex_coap_for_sensor_a(self):
        demo_cfg = Path(__file__).resolve().parents[1] / "backend" / "resources" / "embed-log.demo.yml"
        cfg = load_config(demo_cfg)

        self.assertIn("hex-coap", cfg.frontend_plugins)
        dev_a = cfg.tabs[0]
        self.assertEqual(dev_a.label, "DevA")
        self.assertEqual(dev_a.panes[0].source, "SENSOR_A")
        self.assertEqual([plugin.name for plugin in dev_a.panes[0].plugins], ["hex-coap"])

if __name__ == "__main__":
    unittest.main()
