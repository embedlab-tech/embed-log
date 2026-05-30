from __future__ import annotations

import logging
import socket
import threading

from .raw_base import RawLogSource


class RawUdpSource(RawLogSource):
    def __init__(self, port: int):
        self.port = port

    def start(self, on_chunk, on_boundary, stop: threading.Event, name: str) -> None:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            sock.bind(("0.0.0.0", self.port))
            sock.settimeout(1.0)
        except OSError:
            sock.close()
            raise
        threading.Thread(
            target=self._run,
            args=(sock, on_chunk, on_boundary, stop, name),
            daemon=True,
            name=f"{name}-udp",
        ).start()

    def _run(self, sock: socket.socket, on_chunk, on_boundary, stop: threading.Event, name: str) -> None:
        with sock:
            logging.info("[%s] listening on UDP :%d", name, self.port)
            while not stop.is_set():
                try:
                    data, addr = sock.recvfrom(65535)
                    logging.info("[%s] UDP datagram %d B from %s:%s", name, len(data), addr[0], addr[1])
                    on_chunk(data)
                    on_boundary()
                except socket.timeout:
                    continue
