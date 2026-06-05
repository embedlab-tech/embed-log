"""Abstract capture backend protocol."""

from __future__ import annotations

import threading
from abc import ABC, abstractmethod
from typing import Callable


class CaptureBackend(ABC):
    """Abstract interface for a packet capture backend.

    Implementations MUST call *on_packet(pkt)* for every captured packet during
    `run()`.  Multi-backend support is an explicit goal: subclass this for
    Scapy, PyShark, raw libpcap, or any other capture engine.
    """

    @abstractmethod
    def run(self, on_packet: Callable[[object], None], stop: threading.Event) -> None:
        """Block until *stop* is set, calling *on_packet(pkt)* per captured packet.

        *pkt* is implementation-specific; the normalizer is responsible for
        converting it into the application's internal model.
        """

    @abstractmethod
    def set_filter(self, bpf_filter: str) -> None:
        """Update the active BPF filter.

        Implementations SHOULD apply the new filter on the next available
        opportunity (e.g. between sniff iterations).  If the backend does not
        support live filter changes it MAY raise NotImplementedError; callers
        are expected to restart the capture as a fallback.
        """

    @abstractmethod
    def close(self) -> None:
        """Release all resources (sockets, PCAP writers, threads)."""

    @property
    @abstractmethod
    def interface(self) -> str:
        """Return the interface name this backend is bound to."""

    @property
    @abstractmethod
    def active_filter(self) -> str:
        """Return the currently active BPF filter string."""
