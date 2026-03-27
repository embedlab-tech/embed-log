"""
TxClient — send serial TX data to a running embed-log source channel.

This client is intentionally focused on TX only. For marker/log injection and
stream subscription use backend.log_client.LogClient.
"""

import json
import logging
import socket
import threading
import time
from typing import Optional


class TxClient:
    """Thread-safe TX-only client for one source channel on the log server."""

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 5001,
        source: str = "TEST",
        auto_reconnect: bool = True,
        connect_timeout: float = 0,
    ):
        self._host = host
        self._port = port
        self._source = source
        self._auto_reconnect = auto_reconnect
        self._connect_timeout = connect_timeout
        self._sock: Optional[socket.socket] = None
        self._lock = threading.Lock()

    def __enter__(self) -> "TxClient":
        self.connect()
        return self

    def __exit__(self, *_) -> None:
        self.close()

    def connect(self) -> None:
        with self._lock:
            self._connect_locked()

    def _connect_locked(self) -> None:
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None

        deadline = time.monotonic() + self._connect_timeout
        retry_interval = 1.0
        attempt = 0
        while True:
            attempt += 1
            try:
                sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                sock.connect((self._host, self._port))
                self._sock = sock
                if attempt > 1:
                    logging.info("[TxClient] connected to %s:%d (attempt %d)",
                                 self._host, self._port, attempt)
                return
            except OSError as exc:
                sock.close()
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ConnectionRefusedError(
                        f"[TxClient] could not connect to {self._host}:{self._port} "
                        f"after {self._connect_timeout} s: {exc}"
                    ) from exc
                logging.info(
                    "[TxClient] waiting for server at %s:%d (attempt %d, %.0f s remaining)…",
                    self._host, self._port, attempt, remaining,
                )
                time.sleep(min(retry_interval, remaining))

    def close(self) -> None:
        with self._lock:
            if self._sock is not None:
                try:
                    self._sock.close()
                except OSError:
                    pass
                self._sock = None

    def _send_locked(self, data: bytes) -> None:
        for attempt in range(2):
            try:
                if self._sock is None:
                    raise OSError("not connected")
                self._sock.sendall(data)
                return
            except OSError:
                if attempt == 0 and self._auto_reconnect:
                    self._connect_locked()
                else:
                    raise

    def send(self, data: str | bytes, *, source: Optional[str] = None) -> None:
        """Send raw bytes or text to serial TX."""
        if isinstance(data, bytes):
            data = data.decode("utf-8", errors="replace")
        payload = json.dumps({
            "type": "tx",
            "source": source or self._source,
            "data": data,
        }).encode("utf-8") + b"\n"
        with self._lock:
            self._send_locked(payload)

    def sendline(self, text: str, *, eol: str = "\r\n",
                 source: Optional[str] = None) -> None:
        """Send one line to serial TX."""
        self.send(text + eol, source=source)
