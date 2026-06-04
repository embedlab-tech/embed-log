"""Mock network capture source for deterministic demos.

Generates realistic-looking packet events without requiring Scapy or
packet capture permissions.  Produces a repeatable sequence of events
suitable for testing and demonstration.
"""

from __future__ import annotations

import json
import logging
import random
import threading
import time
from typing import Callable

from .base import LogSource

log = logging.getLogger(__name__)

# Pre-defined "packet flows" for deterministic output
_FLOWS = [
    # Device (192.168.1.100:5683) ↔ Server (192.168.1.1:5683) — CoAP exchange
    ("192.168.1.100", "192.168.1.1", 5683, 5683, "UDP", "CoAP GET /sensors/temp"),
    ("192.168.1.1", "192.168.1.100", 5683, 5683, "UDP", "CoAP 2.05 Content"),
    ("192.168.1.100", "192.168.1.1", 5683, 5683, "UDP", "CoAP POST /actuators/led"),
    ("192.168.1.1", "192.168.1.100", 5683, 5683, "UDP", "CoAP 2.04 Changed"),
    # Occasional mDNS broadcast from device
    ("192.168.1.100", "224.0.0.251", 5353, 5353, "UDP", "mDNS query _coap._udp.local"),
    # DHCP renew
    ("0.0.0.0", "255.255.255.255", 68, 67, "UDP", "DHCP Request"),
    ("192.168.1.1", "192.168.1.100", 67, 68, "UDP", "DHCP ACK"),
    # ICMP ping
    ("192.168.1.100", "192.168.1.1", None, None, "ICMP", "Echo request"),
]


class MockNetworkCaptureSource(LogSource):
    """Generates deterministic fake packet events at a regular interval.

    No actual packet capture is performed.  Events are JSON-serialized
    NormalizedPacket-compatible dicts emitted via the standard on_line
    callback.
    """

    def __init__(self, interface: str = "mock0", interval: float = 0.5) -> None:
        self._interface = interface
        self._interval = interval
        self._thread: threading.Thread | None = None
        self._on_line: Callable[[str], None] | None = None

    # ------------------------------------------------------------------
    # LogSource interface
    # ------------------------------------------------------------------

    def start(self, on_line, stop: threading.Event, name: str) -> None:
        self._on_line = on_line
        self._thread = threading.Thread(
            target=self._run,
            args=(stop, name),
            daemon=True,
            name=f"{name}-mock-net",
        )
        self._thread.start()
        log.info(
            "[%s] mock network capture started  iface=%s  interval=%.2fs",
            name, self._interface, self._interval,
        )

    def close(self) -> None:
        thread = self._thread
        self._thread = None
        if thread and thread.is_alive():
            thread.join(timeout=1.0)

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _run(self, stop: threading.Event, name: str) -> None:
        tick = 0
        rng = random.Random(42)  # deterministic seed
        while not stop.is_set():
            flow = _FLOWS[tick % len(_FLOWS)]
            src_ip, dst_ip, src_port, dst_port, proto, desc = flow
            length = rng.randint(64, 128) if proto == "UDP" else rng.randint(84, 256)
            ts = time.time()

            # Build a payload preview (a few hex bytes)
            payload = bytes([tick % 256, (tick * 7) % 256, 0xAB, 0xCD,
                             (tick * 13) % 256, (tick * 17) % 256])
            payload_hex = " ".join(f"{b:02x}" for b in payload[:16])
            payload_ascii = "".join(chr(b) if 32 <= b < 127 else "." for b in payload[:16])

            src_label = f"{src_ip}" + (f":{src_port}" if src_port else "")
            dst_label = f"{dst_ip}" + (f":{dst_port}" if dst_port else "")
            summary = f"{proto} {src_label} → {dst_label} [{desc}] len={length}"

            event = {
                "timestamp": ts,
                "source": "network_capture",
                "source_name": name,
                "interface": self._interface,
                "length": length,
                "protocol": proto,
                "src": src_ip,
                "dst": dst_ip,
                "src_port": src_port,
                "dst_port": dst_port,
                "summary": summary,
                "payload_hex": payload_hex,
                "payload_ascii": payload_ascii,
                "pcap_file": None,
            }

            on_line = self._on_line
            if on_line is not None:
                on_line(json.dumps(event, separators=(",", ":"), default=str))

            tick += 1
            stop.wait(self._interval)
