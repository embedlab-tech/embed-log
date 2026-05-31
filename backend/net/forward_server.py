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
        self._srv: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self._lock = threading.Lock()

    def start(self) -> None:
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            srv.bind((self._host, self._port))
            srv.listen(16)
            srv.settimeout(1.0)
        except OSError:
            srv.close()
            raise
        thread = threading.Thread(
            target=self._loop,
            args=(srv,),
            daemon=True,
            name=f"{self._name}-fwd-{self._port}",
        )
        with self._lock:
            self._srv = srv
            self._thread = thread
        thread.start()

    def stop(self) -> None:
        with self._lock:
            srv = self._srv
            thread = self._thread
            self._srv = None
            self._thread = None
        if srv is not None:
            try:
                srv.close()
            except OSError:
                pass
        if thread and thread.is_alive():
            thread.join(timeout=2.0)

    def _loop(self, srv: socket.socket) -> None:
        with srv:
            while not self._stop.is_set():
                try:
                    conn, addr = srv.accept()
                except socket.timeout:
                    continue
                except OSError:
                    if self._stop.is_set() or srv.fileno() < 0:
                        break
                    raise
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