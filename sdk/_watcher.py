"""Watcher — regex pattern matching on an embed-log ForwardClient stream."""

from __future__ import annotations

import logging
import re
import threading
import time
from typing import Callable

from ._forward import ForwardClient
from ._models import LogEntry, WatchMatch

_log = logging.getLogger(__name__)


class Watcher:
    """Watch a :class:`ForwardClient` stream for regex patterns.

    Patterns are compiled with :py:func:`re.compile` and matched against
    each incoming :class:`LogEntry.message`.  When a pattern matches a
    callback is invoked with a :class:`WatchMatch`.

    Basic usage::

        fwd = ForwardClient(port=5001)
        fwd.connect()

        watcher = Watcher(fwd, patterns={
            "fatal": r"ZEPHYR FATAL ERROR",
            "timeout": r"watchdog: (?P<seconds>\\d+)s",
        })

        @watcher.on("fatal")
        def on_fatal(m: WatchMatch):
            print(f"HALT: {m.entry.message}")

        @watcher.on_match
        def on_any(m: WatchMatch):
            print(f"[{m.name}] matched")

        watcher.start()          # blocks until stop() or timeout

    Parameters
    ----------
    client:
        An already-connected :class:`ForwardClient`.
    patterns:
        Dict mapping user-defined names to regex strings.
    timeout:
        If set, :meth:`start` returns after this many seconds with no
        matches (idle timeout).  ``None`` means run forever.
    """

    def __init__(
        self,
        client: ForwardClient,
        patterns: dict[str, str],
        *,
        timeout: float | None = None,
    ) -> None:
        self._client = client
        self._patterns = {
            name: re.compile(pattern)
            for name, pattern in patterns.items()
        }
        self._timeout = timeout
        self._callbacks: dict[str | None, list[Callable[[WatchMatch], None]]] = {}
        self._any_callbacks: list[Callable[[WatchMatch], None]] = []
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    # -- decorators ------------------------------------------------------------

    def on(self, name: str) -> Callable:
        """Decorator: register a callback for a specific pattern *name*.

        Usage::

            @watcher.on("fatal")
            def handle_fatal(match: WatchMatch):
                ...
        """
        def decorator(fn: Callable[[WatchMatch], None]) -> Callable[[WatchMatch], None]:
            self._callbacks.setdefault(name, []).append(fn)
            return fn
        return decorator

    def on_match(self, fn: Callable[[WatchMatch], None]) -> Callable[[WatchMatch], None]:
        """Decorator: register a callback for **any** pattern match.

        Usage::

            @watcher.on_match
            def handle_any(match: WatchMatch):
                ...
        """
        self._any_callbacks.append(fn)
        return fn

    # -- lifecycle -------------------------------------------------------------

    def start(self) -> None:
        """Block until :meth:`stop` is called or *timeout* expires.

        Iterates over the :class:`ForwardClient` stream, testing each
        :class:`LogEntry` against all patterns.
        """
        last_match = time.monotonic()
        read_to = min((self._timeout or 1.0) * 0.5, 0.5)
        while not self._stop.is_set():
            if self._timeout is not None and time.monotonic() - last_match > self._timeout:
                _log.debug("Watcher idle timeout after %.1fs", self._timeout)
                break
            entry = self._client.read(timeout=read_to)
            if entry is None:
                continue
            if self._check(entry):
                last_match = time.monotonic()

    def start_background(self) -> threading.Thread:
        """Start watching in a daemon thread.  Returns the thread."""
        self._stop.clear()
        self._thread = threading.Thread(
            target=self.start,
            daemon=True,
            name="Watcher",
        )
        self._thread.start()
        return self._thread

    def stop(self) -> None:
        """Signal the watcher to stop.  Does not block."""
        self._stop.set()

    def wait_for(
        self,
        name: str,
        timeout: float | None = None,
    ) -> WatchMatch | None:
        """Block until pattern *name* matches, or *timeout* expires.

        Returns the :class:`WatchMatch` on success, *None* on timeout.
        """
        result: WatchMatch | None = None
        event = threading.Event()

        def _cb(m: WatchMatch) -> None:
            nonlocal result
            result = m
            event.set()

        self._callbacks.setdefault(name, []).append(_cb)
        t = self.start_background()
        matched = event.wait(timeout=timeout)
        self.stop()
        t.join(timeout=2.0)
        return result if matched else None

    # -- matching --------------------------------------------------------------

    def _check(self, entry: LogEntry) -> bool:
        """Test *entry* against all patterns.  Return True if any matched."""
        any_matched = False
        for name, rx in self._patterns.items():
            m = rx.search(entry.message)
            if m is None:
                continue
            any_matched = True
            match = WatchMatch(
                name=name,
                pattern=rx.pattern,
                entry=entry,
                match=m,
                groups=m.groupdict(),
            )
            # per-pattern callbacks
            for cb in self._callbacks.get(name, []):
                try:
                    cb(match)
                except Exception:
                    _log.exception("Watcher callback for %r raised", name)
            # any-match callbacks
            for cb in self._any_callbacks:
                try:
                    cb(match)
                except Exception:
                    _log.exception("Watcher on_match callback raised")
        return any_matched
