from __future__ import annotations

import logging
import os
import threading
from pathlib import Path
from typing import Callable

from watchdog.events import FileSystemEventHandler
from watchdog.observers import Observer

from .raw_base import RawLogSource


class RawFileSource(RawLogSource):
    """Watches a file for appended lines using watchdog (inotify/kqueue/ReadDirectoryChangesW).

    On every FileModifiedEvent the reader seeks from the last known position to
    EOF and emits any new bytes via *on_chunk*.  If the file shrinks (truncation
    or rotation) the position resets to 0.
    """

    def __init__(self, path: str) -> None:
        self.path = Path(path).resolve()
        self._observer: Observer | None = None
        self._fd: object | None = None         # IO object
        self._pos: int = 0                      # last-read byte offset
        self._fd_lock = threading.Lock()

    # -- RawLogSource interface ------------------------------------------------

    def start(
        self,
        on_chunk: Callable[[bytes], None],
        on_boundary: Callable[[], None],
        stop: threading.Event,
        name: str,
    ) -> None:
        self._on_chunk = on_chunk
        self._on_boundary = on_boundary
        threading.Thread(
            target=self._run,
            args=(stop, name),
            daemon=True,
            name=f"{name}-file",
        ).start()

    # -- watchdog loop ---------------------------------------------------------

    def _run(self, stop: threading.Event, name: str) -> None:
        self._observer = Observer()
        watch_dir = str(self.path.parent)
        handler = _FileEventHandler(self, name)

        # Watch the parent directory so we detect creation / deletion / moves
        self._observer.schedule(handler, watch_dir, recursive=False)
        self._observer.start()
        logging.info("[%s] watching %s", name, self.path)

        # If the file already exists, open it at the end (tail-follow mode).
        if self.path.is_file():
            self._open_append()

        try:
            while not stop.is_set():
                stop.wait(0.5)
        finally:
            self._observer.stop()
            self._observer.join(timeout=3)
            self._close_fd()
            if self._on_boundary is not None:
                self._on_boundary()

    # -- file helpers ----------------------------------------------------------

    def _open_append(self) -> None:
        """Open the file and seek to the end (tail-follow)."""
        self._close_fd()
        try:
            fd = open(self.path, "rb")
            fd.seek(0, os.SEEK_END)
            with self._fd_lock:
                self._fd = fd
                self._pos = fd.tell()
        except OSError as exc:
            logging.warning("cannot open %s: %s", self.path, exc)

    def _open_start(self) -> None:
        """Open the file from the beginning (truncation / rotation)."""
        self._close_fd()
        try:
            fd = open(self.path, "rb")
            with self._fd_lock:
                self._fd = fd
                self._pos = 0
        except OSError as exc:
            logging.warning("cannot open %s: %s", self.path, exc)

    def _close_fd(self) -> None:
        with self._fd_lock:
            if self._fd is not None:
                try:
                    self._fd.close()
                except OSError:
                    pass
                self._fd = None
                self._pos = 0

    def _read_new(self) -> None:
        """Read any new bytes since the last known position and emit them."""
        with self._fd_lock:
            fd = self._fd
            pos = self._pos
        if fd is None:
            return

        try:
            fd.seek(0, os.SEEK_END)
            end = fd.tell()
            if end < pos:
                # File was truncated — reset.
                self._open_start()
                return
            if end == pos:
                return  # nothing new

            fd.seek(pos)
            new_bytes = end - pos
            # Read in 64 KB chunks to avoid giant allocations
            while new_bytes > 0:
                chunk = fd.read(min(new_bytes, 65536))
                if not chunk:
                    break
                new_bytes -= len(chunk)
                self._on_chunk(chunk)

            with self._fd_lock:
                if self._fd is fd:
                    self._pos = fd.tell()
        except (OSError, ValueError) as exc:
            logging.debug("read error on %s: %s", self.path, exc)


class _FileEventHandler(FileSystemEventHandler):
    """Routes watchdog events back to the RawFileSource."""

    def __init__(self, source: RawFileSource, name: str) -> None:
        super().__init__()
        self._source = source
        self._name = name
        self._target = source.path

    def _is_target(self, event_path: str) -> bool:
        # Normalize in case of relative / absolute mismatch.
        try:
            resolved = Path(event_path).resolve()
            match = resolved == self._target
            if not match:
                logging.debug("[%s] watchdog event for non-target: %s (resolved: %s, target: %s)",
                              self._name, event_path, resolved, self._target)
            return match
        except OSError:
            return False

    def on_any_event(self, event) -> None:
        logging.debug("[%s] ANY event: %s src=%s dir=%s", self._name, event.event_type,
                      getattr(event, 'src_path', '?'), event.is_directory)
        super().on_any_event(event)

    def on_modified(self, event) -> None:
        if event.is_directory or not self._is_target(event.src_path):
            return
        logging.debug("[%s] on_modified: %s", self._name, event.src_path)
        self._source._read_new()

    def on_created(self, event) -> None:
        if event.is_directory or not self._is_target(event.src_path):
            return
        logging.info("[%s] file created — opening from start", self._name)
        self._source._open_start()
        self._source._read_new()

    def on_deleted(self, event) -> None:
        if event.is_directory or not self._is_target(event.src_path):
            return
        logging.info("[%s] file deleted — closing", self._name)
        self._source._close_fd()

    def on_moved(self, event) -> None:
        # Handle log rotation: old.log → old.log.1, new old.log appears
        if self._is_target(event.dest_path):
            logging.info("[%s] file moved in — re-opening from start", self._name)
            self._source._open_start()
            self._source._read_new()
        elif self._is_target(event.src_path):
            logging.info("[%s] file moved away — closing", self._name)
            self._source._close_fd()
