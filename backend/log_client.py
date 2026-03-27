"""
LogClient — inject timestamped log markers into a running log-server source
channel, and optionally subscribe to its log stream.

Socket protocol (newline-delimited JSON)
----------------------------------------
    Log marker:  {"type": "log", "source": "...", "message": "...", "color": "cyan"}

    "type" defaults to "log" for backwards compatibility.

    The server also streams every log entry back as JSON lines to each
    connected client — use subscribe() to consume them.

Typical pytest usage
--------------------
    from log_client import LogClient

    @pytest.fixture(scope="session")
    def dut1():
        with LogClient("127.0.0.1", 5001, source="pytest") as client:
            yield client

    def test_boot(dut1):
        dut1.step("resetting board")
        dut1.success("reboot requested")

Subscribe usage (receive log entries without polluting stdout)
--------------------------------------------------------------
    import queue

    log_queue = queue.Queue()
    client.subscribe(log_queue.put)   # background thread; never prints

    entry = log_queue.get(timeout=10)
    assert "boot complete" in entry["message"]

Robot Framework usage
---------------------
    Library    log_client.LogClient    127.0.0.1    5001    source=robot
"""

import json
import logging
import select
import socket
import threading
import time
from typing import Callable, Optional


class LogClient:
    """
    Thread-safe client for one source channel on the log server.

    Parameters
    ----------
    host:
        Hostname or IP of the log server (default: 127.0.0.1).
    port:
        TCP inject port for the target source (matches --inject PORT).
    source:
        Label that appears in log lines, e.g. "test_boot" or "pytest".
    auto_reconnect:
        Reconnect silently if the connection drops (default: True).
    connect_timeout:
        How long to keep retrying the initial connection in seconds.
        0 means fail immediately. Useful when the server may still be
        starting up in CI.
    """

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

    # ------------------------------------------------------------------
    # Context manager
    # ------------------------------------------------------------------

    def __enter__(self) -> "LogClient":
        self.connect()
        return self

    def __exit__(self, *_) -> None:
        self.close()

    # ------------------------------------------------------------------
    # Connection management
    # ------------------------------------------------------------------

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
                    logging.info("[LogClient] connected to %s:%d (attempt %d)",
                                 self._host, self._port, attempt)
                return
            except OSError as exc:
                sock.close()
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ConnectionRefusedError(
                        f"[LogClient] could not connect to {self._host}:{self._port} "
                        f"after {self._connect_timeout} s: {exc}"
                    ) from exc
                logging.info(
                    "[LogClient] waiting for server at %s:%d (attempt %d, %.0f s remaining)…",
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

    # ------------------------------------------------------------------
    # Inject API
    # ------------------------------------------------------------------

    def marker(
        self,
        message: str,
        *,
        color: Optional[str] = None,
        source: Optional[str] = None,
    ) -> None:
        """
        Write a single marker line to the source log.

        Parameters
        ----------
        message:
            Free-form text to log.
        color:
            ANSI color name: red, green, yellow, blue, magenta, cyan, white, bold.
        source:
            Override the source label for this message only.
        """
        payload = json.dumps({
            "source": source or self._source,
            "message": message,
            "color": color,
        }).encode("utf-8") + b"\n"
        with self._lock:
            self._send_locked(payload)

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

    # ------------------------------------------------------------------
    # Subscribe — receive the log stream without printing to stdout
    # ------------------------------------------------------------------

    def subscribe(
        self,
        callback: Callable[[dict], None],
        *,
        daemon: bool = True,
    ) -> threading.Thread:
        """
        Start a background thread that reads log entries streamed back by
        the server and calls ``callback(entry)`` for each one.

        The callback receives a plain dict::

            {
                "source_id": "READER",   # which server source
                "source":    "SERIAL",   # origin within that source
                "message":   "boot ok",
                "timestamp": "2026-03-27T11:49:50.100+01:00",
                "color":     "cyan",     # present only when set
            }

        The callback is called from a dedicated daemon thread and must
        **not** print to stdout — doing so would clobber pytest / Robot
        Framework output.  Hand data back to your test thread via a
        ``queue.Queue``, ``threading.Event``, or similar primitive.

        Parameters
        ----------
        callback:
            Called for every incoming log entry.
        daemon:
            Whether the reader thread is a daemon (default: True).

        Returns
        -------
        threading.Thread
            The started reader thread.  Join it if you need to wait for
            clean shutdown.
        """

        def _reader() -> None:
            buf = b""
            while True:
                with self._lock:
                    sock = self._sock
                if sock is None:
                    return
                try:
                    ready, _, _ = select.select([sock], [], [], 1.0)
                except OSError:
                    return
                if not ready:
                    continue
                try:
                    chunk = sock.recv(4096)
                except OSError:
                    return
                if not chunk:
                    return
                buf += chunk
                while b"\n" in buf:
                    raw, buf = buf.split(b"\n", 1)
                    raw = raw.strip()
                    if not raw:
                        continue
                    try:
                        entry = json.loads(raw)
                        if "message" in entry:
                            callback(entry)
                    except Exception:
                        pass

        t = threading.Thread(target=_reader, daemon=daemon,
                             name="LogClient-subscriber")
        t.start()
        return t

    # ------------------------------------------------------------------
    # Convenience wrappers
    # ------------------------------------------------------------------

    def info(self, message: str) -> None:
        self.marker(message, color="white")

    def success(self, message: str) -> None:
        self.marker(message, color="green")

    def warning(self, message: str) -> None:
        self.marker(message, color="yellow")

    def error(self, message: str) -> None:
        self.marker(message, color="red")

    def step(self, message: str) -> None:
        """Highlight a test step in cyan."""
        self.marker(message, color="cyan")
