"""InjectClient — send log entries, markers, and TX commands to embed-log."""

from __future__ import annotations

import json
import logging
import socket
import threading
import time

_log = logging.getLogger(__name__)


class InjectClient:
    """Connect to an embed-log inject port and send entries.

    The inject port accepts newline-delimited JSON objects.  The wire
    format is:

    ``{"type": "log", "source": "...", "message": "...", "color": "..."}``
    ``{"type": "tx",  "source": "...", "data":    "..."}``

    Basic usage::

        with InjectClient(port=5001, source="pytest") as inj:
            inj.log("test started")
            inj.marker("phase: init", color="cyan")
            inj.tx("version\\r\\n")   # only for UART sources
    """

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 5001,
        source: str = "TEST",
        *,
        auto_reconnect: bool = True,
        connect_timeout: float = 0,
    ) -> None:
        self._host = host
        self._port = port
        self._source = source
        self._auto_reconnect = auto_reconnect
        self._connect_timeout = connect_timeout
        self._sock: socket.socket | None = None
        self._lock = threading.Lock()

    # -- context manager -------------------------------------------------------

    def __enter__(self) -> "InjectClient":
        self.connect()
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- connection ------------------------------------------------------------

    def connect(self) -> None:
        """Open a TCP connection to the inject port."""
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
        retry = 1.0
        while True:
            sock = None
            try:
                sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                sock.settimeout(1.0)
                sock.connect((self._host, self._port))
                self._sock = sock
                return
            except OSError:
                if sock:
                    sock.close()
                if self._connect_timeout <= 0:
                    raise
                if time.monotonic() >= deadline:
                    raise ConnectionError(
                        f"could not connect to {self._host}:{self._port} "
                        f"within {self._connect_timeout}s"
                    )
                time.sleep(retry)
                retry = min(retry * 1.5, 10.0)

    def close(self) -> None:
        """Close the connection."""
        with self._lock:
            if self._sock is not None:
                try:
                    self._sock.close()
                except OSError:
                    pass
                self._sock = None

    # -- sending ---------------------------------------------------------------

    def _send_json(self, payload: dict) -> None:
        """Send a single JSON line.  Reconnects transparently if needed."""
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8") + b"\n"
        with self._lock:
            if self._sock is None:
                if self._auto_reconnect:
                    self._connect_locked()
                else:
                    raise ConnectionError("InjectClient is not connected")
            try:
                self._sock.sendall(data)
            except OSError:
                if self._auto_reconnect:
                    self._connect_locked()
                    self._sock.sendall(data)
                else:
                    raise

    def log(self, message: str, *, color: str | None = None) -> None:
        """Inject a plain log line.

        Appears in the pane as a normal received line with the given
        *color* (one of ``"green"``, ``"yellow"``, ``"red"``, ``"cyan"``,
        ``"white"``).
        """
        payload: dict = {
            "type": "log",
            "source": self._source,
            "message": message,
        }
        if color:
            payload["color"] = color
        self._send_json(payload)

    def tx(self, data: str) -> None:
        """Send a TX (transmit) command to the serial device.

        Only works when the source is a UART source with write support.
        The *data* should include any required line terminator (e.g.
        ``"version\\r\\n"``).
        """
        self._send_json({
            "type": "tx",
            "source": self._source,
            "data": data,
        })

    def marker(self, message: str, *, color: str = "white") -> None:
        """Inject a styled marker line (shortcut for ``log(…, color=…)``)."""
        self.log(message, color=color)

    def info(self, message: str) -> None:
        self.marker(message, color="white")

    def success(self, message: str) -> None:
        self.marker(message, color="green")

    def warning(self, message: str) -> None:
        self.marker(message, color="yellow")

    def error(self, message: str) -> None:
        self.marker(message, color="red")

    def step(self, message: str) -> None:
        self.marker(message, color="cyan")
