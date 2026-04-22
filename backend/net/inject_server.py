from __future__ import annotations

import logging
import select
import socket
import threading
from typing import Callable


class InjectServer:
    def __init__(
        self,
        *,
        name: str,
        host: str,
        port: int,
        stop: threading.Event,
        on_client_connect: Callable[[socket.socket], None],
        on_client_disconnect: Callable[[socket.socket], None],
        on_json_line: Callable[[bytes], None],
    ):
        self._name = name
        self._host = host
        self._port = port
        self._stop = stop
        self._on_client_connect = on_client_connect
        self._on_client_disconnect = on_client_disconnect
        self._on_json_line = on_json_line

    def start(self) -> None:
        threading.Thread(target=self._loop, daemon=True, name=f"{self._name}-inject").start()

    def _loop(self) -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as srv:
            srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            srv.bind((self._host, self._port))
            srv.listen(16)
            srv.settimeout(1.0)
            while not self._stop.is_set():
                try:
                    conn, addr = srv.accept()
                except socket.timeout:
                    continue
                logging.info("[%s] inject client connected: %s:%s", self._name, addr[0], addr[1])
                self._on_client_connect(conn)
                threading.Thread(
                    target=self._handle_client,
                    args=(conn, addr),
                    daemon=True,
                    name=f"{self._name}-client-{addr[1]}",
                ).start()

    def _handle_client(self, conn: socket.socket, addr) -> None:
        buf = b""
        try:
            while not self._stop.is_set():
                try:
                    ready, _, _ = select.select([conn], [], [], 1.0)
                except OSError:
                    break
                if not ready:
                    continue
                try:
                    chunk = conn.recv(4096)
                except OSError:
                    break
                if not chunk:
                    break
                buf += chunk
                while b"\n" in buf:
                    raw_line, buf = buf.split(b"\n", 1)
                    self._on_json_line(raw_line)
        finally:
            self._on_client_disconnect(conn)
            logging.info("[%s] inject client disconnected: %s:%s", self._name, addr[0], addr[1])
            try:
                conn.close()
            except OSError:
                pass
