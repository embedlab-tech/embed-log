"""Packet normalizer — converts raw Scapy packets (or other backend artifacts)
into the application's internal structured event dicts.
"""

from __future__ import annotations

import dataclasses
import time
from typing import Any


@dataclasses.dataclass(slots=True)
class NormalizedPacket:
    """Structured representation of a captured packet ready for serialization."""

    timestamp: float
    source: str
    source_name: str
    interface: str
    length: int
    protocol: str
    src: str | None
    dst: str | None
    src_port: int | None
    dst_port: int | None
    summary: str
    payload_hex: str | None
    payload_ascii: str | None
    pcap_file: str | None

    def to_dict(self) -> dict[str, Any]:
        return dataclasses.asdict(self)


class PacketNormalizer:
    """Converts Scapy packet objects into NormalizedPacket instances.

    Instances are cheap — one per NetworkCaptureSource.
    """

    def __init__(
        self,
        source_name: str,
        interface: str,
        pcap_file: str | None = None,
        include_preview: bool = True,
        max_preview_bytes: int = 128,
    ):
        self.source_name = source_name
        self.interface = interface
        self.pcap_file = pcap_file
        self.include_preview = include_preview
        self.max_preview_bytes = max_preview_bytes

    def normalize(self, pkt: Any) -> NormalizedPacket:
        """Convert a Scapy packet into a NormalizedPacket.

        pkt is expected to be a scapy.packet.Packet (or duck-typed equivalent
        with .time, .wirelen, .summary(), .haslayer(), etc.).
        """
        ts = _packet_time(pkt)
        summary = _safe_str(pkt.summary()) if callable(getattr(pkt, "summary", None)) else ""
        length = getattr(pkt, "wirelen", 0) or len(bytes(pkt))

        proto = _classify_protocol(pkt)
        src_str, dst_str, src_port, dst_port = _extract_addresses(pkt)

        payload_hex, payload_ascii = None, None
        if self.include_preview:
            payload_hex, payload_ascii = _payload_preview(pkt, self.max_preview_bytes)

        return NormalizedPacket(
            timestamp=ts,
            source="network_capture",
            source_name=self.source_name,
            interface=self.interface,
            length=length,
            protocol=proto,
            src=src_str,
            dst=dst_str,
            src_port=src_port,
            dst_port=dst_port,
            summary=summary,
            payload_hex=payload_hex,
            payload_ascii=payload_ascii,
            pcap_file=self.pcap_file,
        )


# ---------------------------------------------------------------------------
# Internal helpers — kept module-private so we can evolve them independently
# ---------------------------------------------------------------------------


def _packet_time(pkt: Any) -> float:
    try:
        return float(pkt.time)
    except (AttributeError, TypeError, ValueError):
        return time.time()


def _safe_str(value: Any) -> str:
    try:
        return str(value)
    except Exception:
        return ""


def _classify_protocol(pkt: Any) -> str:
    """Best-effort protocol classification from Scapy layer inspection."""
    # Scapy layers: check by common names
    layers = []
    try:
        while pkt:
            name = getattr(pkt, "name", None)
            if name:
                layers.append(name.upper())
            pkt = getattr(pkt, "payload", None)
    except Exception:
        pass

    # Priority ordering for the summary protocol label
    priority = ("TCP", "UDP", "ICMP", "ARP", "IP", "IPv6", "Ether")
    for p in priority:
        if p in layers:
            return p
    # Fallback: return the first non-Ether layer, or "Unknown"
    for layer in layers:
        if layer not in ("ETHERNET", "RAW"):
            return layer
    return layers[0] if layers else "Unknown"


def _extract_addresses(pkt: Any) -> tuple[str | None, str | None, int | None, int | None]:
    """Extract source/destination addresses and ports from a Scapy packet."""
    src_str: str | None = None
    dst_str: str | None = None
    src_port: int | None = None
    dst_port: int | None = None

    # IP layer
    if hasattr(pkt, "haslayer"):
        try:
            ip = pkt.getlayer("IP")
        except Exception:
            ip = None
        if ip is not None:
            src_str = getattr(ip, "src", None)
            dst_str = getattr(ip, "dst", None)
    else:
        src_str = getattr(pkt, "src", None) or getattr(pkt, "psrc", None)
        dst_str = getattr(pkt, "dst", None) or getattr(pkt, "pdst", None)

    # Transport ports
    sport_attr = getattr(pkt, "sport", None)
    dport_attr = getattr(pkt, "dport", None)
    if sport_attr is not None:
        try:
            src_port = int(sport_attr)
        except (TypeError, ValueError):
            pass
    if dport_attr is not None:
        try:
            dst_port = int(dport_attr)
        except (TypeError, ValueError):
            pass

    return src_str, dst_str, src_port, dst_port


def _payload_preview(pkt: Any, max_bytes: int) -> tuple[str | None, str | None]:
    """Extract bounded hex + ASCII payload preview from the last layer."""
    try:
        raw = bytes(pkt)
    except Exception:
        return None, None

    # Strip L2 header heuristically: skip first 14 bytes for Ethernet
    if len(raw) > 14:
        raw = raw[14:]

    # Skip IP header (20 bytes min) + transport header heuristically
    if len(raw) > 20:
        offset = 20
        # If UDP (8) or TCP (20-60) try to skip the transport header
        if _has_tcp_or_udp(pkt):
            # Rough heuristic: skip IP + transport
            offset = 40  # 20 IP + 20 TCP min
            # Try to compute from IHL and data offset
            try:
                ihl = (raw[0] & 0x0F) * 4 if len(raw) > 0 else 20
                if ihl >= 20:
                    offset = ihl
                    if hasattr(pkt, "dataofs") and getattr(pkt, "dataofs", None) is not None:
                        offset += getattr(pkt, "dataofs") * 4
            except Exception:
                pass
        payload = raw[offset:]
    else:
        payload = raw

    if not payload:
        return None, None

    payload = payload[:max_bytes]

    hex_parts = []
    ascii_parts = []
    for i in range(0, len(payload), 16):
        chunk = payload[i : i + 16]
        hex_parts.append(" ".join(f"{b:02x}" for b in chunk))
        ascii_parts.append("".join(chr(b) if 32 <= b < 127 else "." for b in chunk))

    return "  ".join(hex_parts), " ".join(ascii_parts)


def _has_tcp_or_udp(pkt: Any) -> bool:
    try:
        return pkt.haslayer("TCP") or pkt.haslayer("UDP")
    except Exception:
        return False
