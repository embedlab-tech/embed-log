import tempfile
import unittest
from pathlib import Path

from backend.config import ConfigError, load_config


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
  job_id: CI-42
logs:
  dir: logs/
baudrate: 115200
sources:
  - name: UART_A
    type: uart
    port: /dev/ttyUSB0
    inject_port: 5001
    forward_ports: [7001, 7002]
  - name: UDP_A
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

        self.assertEqual(cfg["host"], "127.0.0.1")
        self.assertEqual(cfg["ws_port"], 8080)
        self.assertEqual(cfg["app_name"], "demo")
        self.assertTrue(cfg["open_browser"])
        self.assertEqual(cfg["verbosity"], "events")
        self.assertEqual(cfg["job_id"], "CI-42")
        self.assertEqual(cfg["log_dir"], "logs/")
        self.assertEqual(len(cfg["sources"]), 2)
        self.assertEqual(len(cfg["injects"]), 1)
        self.assertEqual(len(cfg["forwards"]), 2)
        self.assertEqual(len(cfg["tabs"]), 1)

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


if __name__ == "__main__":
    unittest.main()
