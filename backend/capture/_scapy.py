"""Scapy-based capture backend.

Uses scapy.all.sniff with store=False in a dedicated thread to capture
packets non-blockingly.  Writes captured packets to a .pcap file via
PcapWriter.

On platforms where Scapy or libpcap/Npcap are unavailable, import of this
module will raise ImportError with a user-friendly message.
"""

from __future__ import annotations

import logging
import threading
from typing import Callable

from ._backend import CaptureBackend

log = logging.getLogger(__name__)

_SCAPY_AVAILABLE = False
_SCAPY_IMPORT_ERROR: str | None = None

try:
    import scapy.all as _scapy  # noqa: F401

    _SCAPY_AVAILABLE = True
except ImportError as exc:
    _SCAPY_IMPORT_ERROR = (
        "Scapy is not installed. Install it with:\n"
        "  pip install scapy\n"
        "On Windows you also need Npcap: https://npcap.com/\n"
        f"Import error: {exc}"
    )


class ScapyCaptureBackend(CaptureBackend):
    """Captures packets using Scapy's sniff() in a background thread.

    Parameters
    ----------
    interface:
        Network interface name (e.g. "eth0", "en0", "lo").
    bpf_filter:
        Initial BPF/PCAP filter string.  Pass "" for no filter.
    pcap_path:
        File path for the output .pcap file, or None to skip writing.
    """

    def __init__(
        self,
        interface: str,
        bpf_filter: str = "",
        pcap_path: str | None = None,
    ) -> None:
        if not _SCAPY_AVAILABLE:
            raise ImportError(_SCAPY_IMPORT_ERROR or "Scapy not available")
        self._interface = interface
        self._bpf_filter = bpf_filter
        self._pcap_path = pcap_path
        self._pcap_writer: object | None = None
        self._sniff_thread: threading.Thread | None = None
        self._filter_lock = threading.Lock()
        self._pending_filter: str | None = None
        self._sniff_socket: object | None = None

    # ------------------------------------------------------------------
    # CaptureBackend interface
    # ------------------------------------------------------------------

    @property
    def interface(self) -> str:
        return self._interface

    @property
    def active_filter(self) -> str:
        return self._bpf_filter

    def set_filter(self, new_filter: str) -> None:
        """Queue a BPF filter change.

        Scapy's sniff does not expose a simple live-filter-change API, so we
        signal the sniff loop to restart with the new filter.  This is
        transparent to the caller.
        """
        with self._filter_lock:
            self._pending_filter = new_filter

    def run(self, on_packet: Callable[[object], None], stop: threading.Event) -> None:
        """Block until *stop* is set, calling *on_packet* for every captured packet."""
        # Open L2 socket so we can close it to interrupt sniff()
        self._open_sniff_socket()
        pcap = self._open_pcap()

        # Sniff loop: may restart when the filter changes
        while not stop.is_set():
            current_filter = self._bpf_filter
            self._sniff_loop(on_packet, stop, current_filter, pcap)

            # Check for pending filter change
            with self._filter_lock:
                new_filter = self._pending_filter
                self._pending_filter = None

            if new_filter is not None and new_filter != self._bpf_filter:
                log.info("[network_capture] updating filter %r → %r on %s",
                         self._bpf_filter, new_filter, self._interface)
                self._bpf_filter = new_filter

    def close(self) -> None:
        """Release the sniff socket and close the PCAP writer."""
        sock = self._sniff_socket
        if sock is not None:
            try:
                sock.close()
            except Exception:
                pass
            self._sniff_socket = None

        pcap = self._pcap_writer
        if pcap is not None:
            try:
                pcap.close()  # type: ignore[union-attr]
            except Exception:
                pass
            self._pcap_writer = None

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _open_sniff_socket(self) -> None:
        """Open a L2 socket that sniff() will use; stored so we can close it."""
        try:
            self._sniff_socket = _scapy.conf.L2listen(
                iface=self._interface, filter=self._bpf_filter or None
            )
        except Exception as exc:
            raise OSError(
                f"Cannot open capture interface {self._interface!r}: {exc}. "
                "Check permissions (Linux: sudo, or set CAP_NET_RAW; Windows: install Npcap)."
            ) from exc

    def _open_pcap(self) -> object | None:
        if not self._pcap_path:
            return None
        try:
            from pathlib import Path
            Path(self._pcap_path).parent.mkdir(parents=True, exist_ok=True)
            writer = _scapy.PcapWriter(self._pcap_path, append=False, sync=True)
            self._pcap_writer = writer
            log.info("[network_capture] writing PCAP to %s", self._pcap_path)
            return writer
        except Exception as exc:
            log.warning("[network_capture] cannot open PCAP %s: %s", self._pcap_path, exc)
            return None

    def _sniff_loop(
        self,
        on_packet: Callable[[object], None],
        stop: threading.Event,
        bpf_filter: str,
        pcap_writer: object | None,
    ) -> None:
        """Run one sniff() call.  Returns when *stop* is set or the socket closes."""

        def _callback(pkt: object) -> None:
            if pcap_writer is not None:
                try:
                    pcap_writer.write(pkt)  # type: ignore[union-attr]
                except Exception:
                    pass
            on_packet(pkt)

        try:
            _scapy.sniff(
                iface=self._interface,
                filter=bpf_filter or None,
                prn=_callback,
                store=False,
                timeout=1,  # wake up every second to check stop
                opened_socket=self._sniff_socket,
                stop_filter=lambda _: stop.is_set(),
            )
        except Exception as exc:
            if stop.is_set():
                return
            log.warning("[network_capture] sniff error on %s: %s — retrying in 3 s",
                        self._interface, exc)
            stop.wait(3)
