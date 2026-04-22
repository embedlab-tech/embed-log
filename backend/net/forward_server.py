from __future__ import annotations

import logging
import socket
import threading
from typing import Callable


class ForwardServer:
    def __init__(
        self,
        *,
        name: str,
        host: str,
        port: int,
        stop: threading.Event,
        on_client_connect: Callable[[socket.socket], None],
        on_client_disconnect: Callable[[socket.socket], None],
    ):
        self._name = name
        self._host = host
        self._port = port
        self._stop = stop
        self._on_client_connect = on_client_connect
        self._on_client_disconnect = on_client_disconnect

    def start(self) -> None:
        threading.Thread(
            target=self._loop,
            daemon=True,
            name=f"{self._name}-fwd-{self._port}",
        ).start()

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
                logging.info("[%s] forward client connected on :%d from %s:%s", self._name, self._port, addr[0], addr[1])
                self._on_client_connect(conn)
                threading.Thread(
                    target=self._handle_client,
                    args=(conn, addr),
                    daemon=True,
                    name=f"{self._name}-fwd-client-{addr[1]}",
                ).start()

    def _handle_client(self, conn: socket.socket, addr) -> None:
        try:
            conn.settimeout(1.0)
            while not self._stop.is_set():
                try:
                    data = conn.recv(1)
                except socket.timeout:
                    continue
                except OSError:
                    break
                if not data:
                    break
                # Read-only forwarding socket: ignore any inbound bytes.
        finally:
            self._on_client_disconnect(conn)
            logging.info("[%s] forward client disconnected on :%d from %s:%s", self._name, self._port, addr[0], addr[1])
            try:
                conn.close()
            except OSError:
                pass
