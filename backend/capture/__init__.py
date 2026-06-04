"""Network packet capture backends.

Provides an abstract interface for packet capture backends and the included
Scapy backend.  Additional backends (PyShark/tshark, raw libpcap, etc.) can
be added by implementing the CaptureBackend protocol.
"""

from __future__ import annotations

from ._backend import CaptureBackend
from .normalizer import PacketNormalizer, NormalizedPacket

__all__ = [
    "CaptureBackend",
    "NormalizedPacket",
    "PacketNormalizer",
]
