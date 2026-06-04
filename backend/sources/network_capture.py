"""Network packet capture source.

A LogSource implementation that captures live network packets through a
pluggable CaptureBackend, normalizes them into structured events, and feeds
them into the existing log/event pipeline.
"""

from __future__ import annotations

import json
import logging
import threading
from typing import Callable

from ..capture import CaptureBackend, PacketNormalizer
from ..capture._scapy import ScapyCaptureBackend, _SCAPY_AVAILABLE
from .base import LogSource

log = logging.getLogger(__name__)


class NetworkCaptureSource(LogSource):
    """Captures network packets and emits normalized JSON events.

    Thread-safe for filter changes while running.
    """

    def __init__(
        self,
        interface: str,
        bpf_filter: str = "",
        pcap_enabled: bool = False,
        pcap_path: str | None = None,
        include_preview: bool = True,
        max_preview_bytes: int = 128,
    ) -> None:
        self._interface = interface
        self._bpf_filter = bpf_filter
        self._pcap_enabled = pcap_enabled
        self._pcap_path = pcap_path
        self._include_preview = include_preview
        self._max_preview_bytes = max_preview_bytes

        self._backend: CaptureBackend | None = None
        self._normalizer: PacketNormalizer | None = None
        self._on_line: Callable[[str], None] | None = None
        self._thread: threading.Thread | None = None
        self._filter_lock = threading.Lock()
        self._pending_filter: str | None = None

    # ------------------------------------------------------------------
    # LogSource interface
    # ------------------------------------------------------------------

    def start(self, on_line: Callable[[str], None], stop: threading.Event, name: str) -> None:
        self._on_line = on_line

        # Resolve backend
        if not _SCAPY_AVAILABLE:
            raise RuntimeError(
                "Scapy is not installed. Install it with: pip install scapy\n"
                "On Windows you also need Npcap: https://npcap.com/"
            )

        actual_pcap_path = None
        if self._pcap_enabled and self._pcap_path:
            actual_pcap_path = self._pcap_path

        try:
            self._backend = ScapyCaptureBackend(
                interface=self._interface,
                bpf_filter=self._bpf_filter,
                pcap_path=actual_pcap_path,
            )
        except OSError as exc:
            raise RuntimeError(
                f"Failed to open capture interface {self._interface!r}: {exc}"
            ) from exc

        self._normalizer = PacketNormalizer(
            source_name=name,
            interface=self._interface,
            pcap_file=actual_pcap_path,
            include_preview=self._include_preview,
            max_preview_bytes=self._max_preview_bytes,
        )

        self._thread = threading.Thread(
            target=self._run,
            args=(stop, name),
            daemon=True,
            name=f"{name}-capture",
        )
        self._thread.start()

        log.info(
            "[%s] network_capture started  iface=%s  filter=%r  pcap=%s",
            name,
            self._interface,
            self._bpf_filter or "(none)",
            actual_pcap_path or "(none)",
        )

    def close(self) -> None:
        backend = self._backend
        if backend is not None:
            backend.close()
            self._backend = None
        thread = self._thread
        self._thread = None
        if thread and thread.is_alive():
            thread.join(timeout=3.0)

    # ------------------------------------------------------------------
    # Filter management (called from outside the capture thread)
    # ------------------------------------------------------------------

    def set_filter(self, new_filter: str) -> None:
        """Request a BPF filter change.  Returns immediately.

        The filter is validated synchronously (raises on invalid filter syntax);
        the actual application happens asynchronously in the capture loop.
        """
        self._validate_filter(new_filter)
        with self._filter_lock:
            self._pending_filter = new_filter
        backend = self._backend
        if backend is not None:
            backend.set_filter(new_filter)

    @staticmethod
    def _validate_filter(bpf_str: str) -> None:
        """Validate BPF filter syntax.  Raises ValueError on invalid filter.

        Performs a basic sanity check.  Full validation happens at capture
        start time through libpcap/Scapy; this method catches obviously
        malformed strings early.
        """
        if not bpf_str or not bpf_str.strip():
            return  # empty = no filter is always valid
        bpf = bpf_str.strip()

        # Reject strings that look like regex to catch accidental regex input.
        # BPF uses . for IP addresses, () for grouping, but *+?[]{}^$ are regex-only.
        for c in "*+?[]{}$":
            if c in bpf:
                raise ValueError(
                    f"BPF filter contains invalid character {c!r}: {bpf!r}. "
                    "BPF filters use pcap-filter syntax (man pcap-filter), not regex."
                )
        # Parentheses are valid in BPF only when balanced
        if bpf.count("(") != bpf.count(")"):
            raise ValueError(
                f"Unbalanced parentheses in BPF filter: {bpf!r}. "
                "BPF filters use pcap-filter syntax (man pcap-filter)."
            )

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _run(self, stop: threading.Event, name: str) -> None:
        backend = self._backend
        normalizer = self._normalizer
        assert backend is not None and normalizer is not None

        def on_packet(pkt: object) -> None:
            try:
                normalized = normalizer.normalize(pkt)
                event_json = json.dumps(normalized.to_dict(), separators=(",", ":"), default=str)
            except Exception:
                log.debug("[%s] failed to normalize packet", name, exc_info=True)
                return
            if self._on_line is not None:
                self._on_line(event_json)

        try:
            while not stop.is_set():
                # Check for pending filter changes
                with self._filter_lock:
                    pending = self._pending_filter
                    self._pending_filter = None
                if pending is not None:
                    backend.set_filter(pending)

                backend.run(on_packet, stop)
        except Exception:
            if not stop.is_set():
                log.exception("[%s] capture fatal error", name)
