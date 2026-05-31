from __future__ import annotations

import logging
import socket
import threading

from .raw_base import RawLogSource


class RawUdpSource(RawLogSource):
    def __init__(self, port: int):
        self.port = port
        self._sock: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self._lock = threading.Lock()

    def start(self, on_chunk, on_boundary, stop: threading.Event, name: str) -> None:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            sock.bind(("0.0.0.0", self.port))
            sock.settimeout(1.0)
        except OSError:
            sock.close()
            raise
        thread = threading.Thread(
            target=self._run,
            args=(sock, on_chunk, on_boundary, stop, name),
            daemon=True,
            name=f"{name}-udp",
        )
        with self._lock:
            self._sock = sock
            self._thread = thread
        thread.start()

    def close(self) -> None:
        with self._lock:
            sock = self._sock
            thread = self._thread
            self._sock = None
            self._thread = None
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass
        if thread and thread.is_alive():
            thread.join(timeout=2.0)

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
                except OSError:
                    if stop.is_set() or sock.fileno() < 0:
                        break
                    raise