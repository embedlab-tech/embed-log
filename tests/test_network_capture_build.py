"""Tests for building NetworkCaptureSource through build_source()."""

from __future__ import annotations

import unittest

from backend.app import build_source


class NetworkCaptureBuildTests(unittest.TestCase):
    def test_build_network_capture_source(self) -> None:
        config = {
            "name": "nc0",
            "type": "network_capture",
            "interface": "eth0",
            "bpf_filter": "udp",
            "pcap_enabled": True,
            "pcap_path": "captures/test.pcap",
            "include_preview": False,
            "max_preview_bytes": 64,
        }
        source = build_source(config)
        from backend.sources.network_capture import NetworkCaptureSource
        self.assertIsInstance(source, NetworkCaptureSource)
        self.assertEqual(source._interface, "eth0")
        self.assertEqual(source._bpf_filter, "udp")
        self.assertTrue(source._pcap_enabled)
        self.assertEqual(source._pcap_path, "captures/test.pcap")
        self.assertFalse(source._include_preview)
        self.assertEqual(source._max_preview_bytes, 64)

    def test_build_with_defaults(self) -> None:
        config = {
            "name": "nc1",
            "type": "network_capture",
            "interface": "en0",
        }
        source = build_source(config)
        from backend.sources.network_capture import NetworkCaptureSource
        self.assertIsInstance(source, NetworkCaptureSource)
        self.assertEqual(source._interface, "en0")
        self.assertEqual(source._bpf_filter, "")
        self.assertFalse(source._pcap_enabled)
        self.assertIsNone(source._pcap_path)

    def test_build_unsupported_type_raises(self) -> None:
        config = {"name": "X", "type": "bogus_type", "port": "/dev/tty"}
        with self.assertRaises(ValueError) as ctx:
            build_source(config)
        self.assertIn("unsupported type", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
