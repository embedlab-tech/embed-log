"""Tests for the PacketNormalizer — packet-to-event conversion."""

from __future__ import annotations

import json
import unittest
from typing import Any

from backend.capture.normalizer import (
    NormalizedPacket,
    PacketNormalizer,
    _classify_protocol,
)


class _FakePacket:
    """Minimal duck-typed Scapy-like packet for testing the normalizer."""

    def __init__(
        self,
        time: float = 1710000000.123,
        summary: str = "Test summary",
        wirelen: int = 100,
        src: str | None = None,
        dst: str | None = None,
        sport: int | None = None,
        dport: int | None = None,
        layers: list[str] | None = None,
        raw_bytes: bytes = b"",
    ) -> None:
        self.time = time
        self._summary = summary
        self.wirelen = wirelen
        self.src = src
        self.dst = dst
        self.sport = sport
        self.dport = dport
        self._layers = layers or []
        self._raw = raw_bytes
        # Emulate Scapy layer chain for _classify_protocol
        if self._layers:
            self.name = self._layers[0]
            self.payload = _build_layer_chain(self._layers[1:])
        else:
            self.name = None
            self.payload = None

    def summary(self) -> str:
        return self._summary

    def __bytes__(self) -> bytes:
        return self._raw

    def haslayer(self, name: str) -> bool:
        return name in self._layers

    def getlayer(self, name: str) -> Any:
        if name in self._layers:
            return _FakePacket(
                src=self.src,
                dst=self.dst,
                sport=self.sport,
                dport=self.dport,
                layers=[name],
            )
        return None


def _build_layer_chain(layer_names: list[str]) -> Any:
    """Build a Scapy-like layer chain from a list of layer names."""
    if not layer_names:
        return None
    pkt = _FakePacket()
    pkt.name = layer_names[0]
    pkt.payload = _build_layer_chain(layer_names[1:])
    return pkt


class PacketNormalizerTests(unittest.TestCase):
    def test_normalize_basic_packet(self) -> None:
        norm = PacketNormalizer("test_src", "eth0")
        pkt = _FakePacket(
            time=1710000000.123,
            summary="Ether / IP / UDP 192.168.1.20:5000 > 192.168.1.100:6000",
            wirelen=86,
            src="192.168.1.20",
            dst="192.168.1.100",
            sport=5000,
            dport=6000,
            layers=["Ether", "IP", "UDP"],
        )
        result = norm.normalize(pkt)
        self.assertIsInstance(result, NormalizedPacket)
        d = result.to_dict()
        self.assertEqual(d["timestamp"], 1710000000.123)
        self.assertEqual(d["source"], "network_capture")
        self.assertEqual(d["source_name"], "test_src")
        self.assertEqual(d["interface"], "eth0")
        self.assertEqual(d["length"], 86)
        self.assertEqual(d["protocol"], "UDP")
        self.assertEqual(d["src"], "192.168.1.20")
        self.assertEqual(d["dst"], "192.168.1.100")
        self.assertEqual(d["src_port"], 5000)
        self.assertEqual(d["dst_port"], 6000)
        js = json.dumps(d, default=str)
        self.assertIn("192.168.1.20", js)

    def test_normalize_tcp_packet(self) -> None:
        norm = PacketNormalizer("net", "lo")
        pkt = _FakePacket(
            time=1000.0,
            summary="TCP 10.0.0.1:443 > 10.0.0.2:54321",
            wirelen=1500,
            src="10.0.0.1",
            dst="10.0.0.2",
            sport=443,
            dport=54321,
            layers=["Ether", "IP", "TCP"],
        )
        result = norm.normalize(pkt)
        self.assertEqual(result.protocol, "TCP")
        self.assertEqual(result.length, 1500)

    def test_normalize_arp_packet(self) -> None:
        norm = PacketNormalizer("net", "en0")
        pkt = _FakePacket(
            time=2000.0,
            summary="ARP who-has 10.0.0.2 tell 10.0.0.1",
            wirelen=42,
            layers=["ARP"],
        )
        result = norm.normalize(pkt)
        self.assertEqual(result.protocol, "ARP")
        self.assertEqual(result.length, 42)

    def test_normalize_without_addresses(self) -> None:
        norm = PacketNormalizer("net", "eth0")
        pkt = _FakePacket(time=3000.0, summary="Raw", wirelen=64, layers=[])
        result = norm.normalize(pkt)
        self.assertIsNone(result.src)
        self.assertIsNone(result.dst)
        self.assertIsNone(result.src_port)
        self.assertIsNone(result.dst_port)
        self.assertEqual(result.protocol, "Unknown")

    def test_protocol_classification_priority(self) -> None:
        pkt = _FakePacket(layers=["Ether", "IP", "TCP"])
        self.assertEqual(_classify_protocol(pkt), "TCP")

        pkt = _FakePacket(layers=["Ether", "IP", "UDP"])
        self.assertEqual(_classify_protocol(pkt), "UDP")

        pkt = _FakePacket(layers=["Ether", "IP", "ICMP"])
        self.assertEqual(_classify_protocol(pkt), "ICMP")

    def test_payload_preview_disabled(self) -> None:
        norm = PacketNormalizer("net", "eth0", include_preview=False)
        pkt = _FakePacket(raw_bytes=b"\x00" * 100)
        result = norm.normalize(pkt)
        self.assertIsNone(result.payload_hex)
        self.assertIsNone(result.payload_ascii)

    def test_payload_preview_bounded(self) -> None:
        norm = PacketNormalizer("net", "eth0", max_preview_bytes=32)
        raw = b"\x00" * 14
        raw += b"\x45\x00\x00\x00\x00\x00\x00\x00\x40\x11\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
        raw += b"\x00\x00\x00\x00\x00\x00\x00\x00"
        raw += b"PAYLOAD_DATA_" + b"\x41" * 80
        pkt = _FakePacket(raw_bytes=raw, layers=["IP", "UDP"])
        result = norm.normalize(pkt)
        self.assertIsNotNone(result.payload_hex)
        self.assertIsNotNone(result.payload_ascii)
        hex_str = result.payload_hex or ""
        self.assertLessEqual(len(hex_str.split()), 64)

    def test_pcap_file_in_event(self) -> None:
        norm = PacketNormalizer("net", "eth0", pcap_file="caps/test.pcap")
        pkt = _FakePacket()
        result = norm.normalize(pkt)
        self.assertEqual(result.pcap_file, "caps/test.pcap")

    def test_json_serializable(self) -> None:
        norm = PacketNormalizer("s", "iface")
        pkt = _FakePacket(
            time=1.5,
            summary="test",
            wirelen=10,
            src="1.2.3.4",
            dst="5.6.7.8",
            sport=80,
            dport=443,
            layers=["Ether", "IP", "TCP"],
        )
        d = norm.normalize(pkt).to_dict()
        js = json.dumps(d, default=str)
        roundtrip = json.loads(js)
        self.assertEqual(roundtrip["src"], "1.2.3.4")
        self.assertEqual(roundtrip["dst_port"], 443)

    def test_normalize_tcp_zero_ports(self) -> None:
        norm = PacketNormalizer("net", "lo")
        pkt = _FakePacket(
            src="1.1.1.1", dst="2.2.2.2",
            sport=0, dport=0,
            layers=["Ether", "IP", "TCP"],
        )
        result = norm.normalize(pkt)
        self.assertEqual(result.src_port, 0)
        self.assertEqual(result.dst_port, 0)


if __name__ == "__main__":
    unittest.main()
