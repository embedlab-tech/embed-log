"""Tests for the network_capture config loader support."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from textwrap import dedent

from backend.config import AppConfig, ConfigError, load_config


class NetworkCaptureConfigTests(unittest.TestCase):
    """Verify that network_capture source type is parsed correctly from YAML."""

    def _write_config(self, yaml_text: str) -> str:
        fd, path = tempfile.mkstemp(suffix=".yml", prefix="nc_test_")
        Path(path).write_text(dedent(yaml_text), encoding="utf-8")
        return path

    def test_minimal_network_capture_config(self) -> None:
        yaml = """
        version: 1
        sources:
          - name: net0
            type: network_capture
            interface: eth0
        tabs:
          - label: Net
            panes: [net0]
        """
        cfg = load_config(self._write_config(yaml))
        self.assertEqual(len(cfg.sources), 1)
        src = cfg.sources[0]
        self.assertEqual(src.name, "net0")
        self.assertEqual(src.type, "network_capture")
        self.assertEqual(src.interface, "eth0")
        self.assertEqual(src.bpf_filter, "")
        self.assertFalse(src.pcap_enabled)
        self.assertIsNone(src.pcap_path)
        self.assertTrue(src.include_preview)
        self.assertEqual(src.max_preview_bytes, 128)
        self.assertEqual(len(cfg.tabs), 1)
        self.assertEqual(cfg.tabs[0].panes[0].source, "net0")

    def test_full_network_capture_config(self) -> None:
        yaml = """
        version: 1
        sources:
          - name: device_network
            type: network_capture
            enabled: true
            interface: eth0
            bpf_filter: "udp or tcp"
            pcap:
              enabled: true
              path: "captures/device_network.pcap"
            payload:
              include_preview: false
              max_preview_bytes: 64
        tabs:
          - label: Net
            panes: [device_network]
        """
        cfg = load_config(self._write_config(yaml))
        src = cfg.sources[0]
        self.assertEqual(src.name, "device_network")
        self.assertEqual(src.type, "network_capture")
        self.assertEqual(src.interface, "eth0")
        self.assertEqual(src.bpf_filter, "udp or tcp")
        self.assertTrue(src.pcap_enabled)
        self.assertEqual(src.pcap_path, "captures/device_network.pcap")
        self.assertFalse(src.include_preview)
        self.assertEqual(src.max_preview_bytes, 64)

    def test_network_capture_with_port_is_rejected(self) -> None:
        yaml = """
        version: 1
        sources:
          - name: net0
            type: network_capture
            interface: eth0
            port: 9000
        tabs:
          - label: Net
            panes: [net0]
        """
        with self.assertRaises(ConfigError) as ctx:
            load_config(self._write_config(yaml))
        self.assertIn("port is not used for network_capture", str(ctx.exception))

    def test_network_capture_missing_interface_fails(self) -> None:
        yaml = """
        version: 1
        sources:
          - name: net0
            type: network_capture
        tabs:
          - label: Net
            panes: [net0]
        """
        with self.assertRaises(ConfigError) as ctx:
            load_config(self._write_config(yaml))
        self.assertIn("interface", str(ctx.exception))

    def test_pcap_disabled_no_path_required(self) -> None:
        yaml = """
        version: 1
        sources:
          - name: net0
            type: network_capture
            interface: eth0
            pcap:
              enabled: false
        tabs:
          - label: Net
            panes: [net0]
        """
        cfg = load_config(self._write_config(yaml))
        self.assertFalse(cfg.sources[0].pcap_enabled)
        self.assertIsNone(cfg.sources[0].pcap_path)

    def test_pcap_enabled_without_path_fails(self) -> None:
        yaml = """
        version: 1
        sources:
          - name: net0
            type: network_capture
            interface: eth0
            pcap:
              enabled: true
        tabs:
          - label: Net
            panes: [net0]
        """
        with self.assertRaises(ConfigError) as ctx:
            load_config(self._write_config(yaml))
        self.assertIn("pcap.path", str(ctx.exception))

    def test_network_capture_type_in_error_message(self) -> None:
        yaml = """
        version: 1
        sources:
          - name: net0
            type: unknown_type
            interface: eth0
        tabs:
          - label: Net
            panes: [net0]
        """
        with self.assertRaises(ConfigError) as ctx:
            load_config(self._write_config(yaml))
        self.assertIn("network_capture", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
