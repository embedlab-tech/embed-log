from __future__ import annotations

import logging
import socket
import threading

from .base import LogSource


class UdpSource(LogSource):
    """Listens for UDP datagrams; each datagram may contain multiple newline-separated lines."""

    def __init__(self, port: int):
        self.port = port

    def start(self, on_line, stop: threading.Event, name: str):
        threading.Thread(
            target=self._run, args=(on_line, stop, name),
            daemon=True, name=f"{name}-udp",
        ).start()

    def _run(self, on_line, stop: threading.Event, name: str):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.bind(("0.0.0.0", self.port))
            sock.settimeout(1.0)
            logging.info("[%s] listening on UDP :%d", name, self.port)
            while not stop.is_set():
                try:
                    data, addr = sock.recvfrom(65535)
                    logging.info("[%s] UDP datagram %d B from %s:%s", name, len(data), addr[0], addr[1])
                    for line in data.decode("utf-8", errors="replace").splitlines():
                        if line.strip():
                            on_line(line.rstrip())
                except socket.timeout:
                    continue
