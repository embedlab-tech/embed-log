"""ForwardClient — reads the JSON log stream from an embed-log inject port."""

from __future__ import annotations

import json
import logging
import select
import socket
import threading
import time
from collections import deque
from typing import Iterator

from ._models import LogEntry

_log = logging.getLogger(__name__)


class ForwardClient:
    """Connect to an embed-log inject port and receive log entries as a stream.

    The embed-log inject port is bidirectional: clients send newline-delimited
    JSON to *inject* entries and simultaneously receive a JSON stream of **all**
    log entries (TX and RX) produced by that source.  ``ForwardClient`` only
    reads — it does not inject anything.

    Basic usage::

        with ForwardClient(port=5001) as fwd:
            for entry in fwd:
                print(entry.message)
    """

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 5001,
        *,
        auto_reconnect: bool = True,
        connect_timeout: float = 0,
        buffer_size: int = 65536,
    ) -> None:
        self._host = host
        self._port = port
        self._auto_reconnect = auto_reconnect
        self._connect_timeout = connect_timeout
        self._buffer_size = buffer_size
        self._sock: socket.socket | None = None
        self._lock = threading.Lock()
        self._buf = b""
        self._queue: deque[LogEntry] = deque()
        self._reader_thread: threading.Thread | None = None
        self._stop = threading.Event()

    # -- context manager -------------------------------------------------------

    def __enter__(self) -> "ForwardClient":
        self.connect()
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- connection ------------------------------------------------------------

    def connect(self) -> None:
        """Open a TCP connection to the inject port and start reading."""
        with self._lock:
            self._connect_locked()

    def _connect_locked(self) -> None:
        self._disconnect_locked()
        deadline = time.monotonic() + self._connect_timeout
        retry = 1.0
        while True:
            sock = None
            try:
                sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                sock.settimeout(1.0)
                sock.connect((self._host, self._port))
                self._sock = sock
                self._buf = b""
                self._queue.clear()
                self._stop.clear()
                self._reader_thread = threading.Thread(
                    target=self._reader,
                    daemon=True,
                    name="ForwardClient-reader",
                )
                self._reader_thread.start()
                return
            except OSError:
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
        """Close the connection and stop the reader thread."""
        with self._lock:
            self._disconnect_locked()

    def _disconnect_locked(self) -> None:
        self._stop.set()
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None
        # Wait briefly for reader thread to exit
        if self._reader_thread is not None and self._reader_thread.is_alive():
            self._reader_thread.join(timeout=2.0)
        self._reader_thread = None

    # -- reading ---------------------------------------------------------------

    def _reader(self) -> None:
        """Background thread: read from socket, parse JSON, enqueue entries."""
        buf = b""
        while not self._stop.is_set():
            with self._lock:
                sock = self._sock
            if sock is None:
                return
            try:
                ready, _, _ = select.select([sock], [], [], 0.5)
            except (OSError, ValueError):
                return
            if not ready:
                continue
            try:
                chunk = sock.recv(self._buffer_size)
            except OSError:
                if self._auto_reconnect:
                    _log.debug("ForwardClient read error, reconnecting...")
                    with self._lock:
                        if not self._stop.is_set():
                            try:
                                self._connect_locked()
                            except Exception:
                                time.sleep(1)
                    continue
                return
            if not chunk:
                if self._auto_reconnect:
                    _log.debug("ForwardClient connection closed, reconnecting...")
                    with self._lock:
                        if not self._stop.is_set():
                            try:
                                self._connect_locked()
                            except Exception:
                                time.sleep(1)
                    continue
                return
            buf += chunk
            while b"\n" in buf:
                raw, buf = buf.split(b"\n", 1)
                raw = raw.strip()
                if not raw:
                    continue
                try:
                    payload = json.loads(raw)
                    entry = LogEntry.from_json(payload)
                    with self._lock:
                        self._queue.append(entry)
                except Exception:
                    _log.debug("ForwardClient: malformed JSON line %r", raw[:120])
                    continue

    # -- iteration -------------------------------------------------------------

    def __iter__(self) -> Iterator[LogEntry]:
        """Blocking iterator over incoming log entries.

        Yields each :class:`LogEntry` as it arrives.  Stops when the
        connection is closed or ``close()`` is called from another thread.
        """
        while True:
            with self._lock:
                if self._sock is None and not self._queue:
                    return
                while self._queue:
                    yield self._queue.popleft()
            # Brief sleep to avoid busy-wait when queue is empty
            # but connection is still alive.
            time.sleep(0.05)

    def read(self, timeout: float | None = 0) -> LogEntry | None:
        """Read the next available entry, or *None* if nothing arrives within
        *timeout* seconds.  Pass *timeout* = ``None`` to block indefinitely.
        """
        deadline = None if timeout is None else time.monotonic() + timeout
        while True:
            with self._lock:
                if self._queue:
                    return self._queue.popleft()
                if self._sock is None:
                    return None
            if deadline is not None and time.monotonic() >= deadline:
                return None
            time.sleep(0.02)
