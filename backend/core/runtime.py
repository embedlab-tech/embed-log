from __future__ import annotations

import dataclasses
import json
import logging
import os


import signal
import threading
from datetime import datetime
from pathlib import Path
from typing import Callable, TextIO

import serial

from ..net import ForwardServer, InjectServer, WebSocketBroadcaster
from ..session import SessionExporter, SessionManager
from ..sources import LogSource
from .models import LogEntry, QueueStats
from .ansi import ANSI
from .clock import SessionClock, TIMESTAMP_MODE_ABSOLUTE, TIMESTAMP_MODE_RELATIVE, TIMESTAMP_MODES
from .naming import slugify
from .queue import TrackedQueue


class SourceManager:
    """
    Owns a LogSource, an optional inject TCP server, a write queue,
    and a writer thread.  The inject port is bidirectional: clients can
    inject log markers / TX commands (send JSON lines) and simultaneously
    receive a stream of all log entries for this source.
    """

    def __init__(
        self,
        name: str,
        source: LogSource,
        log_file: str,
        socket_host: str,
        inject_port: int | None = None,
        forward_ports: list[int] | None = None,
        verbose: bool = False,
        broadcaster: WebSocketBroadcaster | None = None,
        queue_maxsize: int = 0,
        session_clock: SessionClock | None = None,
        clock: Callable[[], datetime] | None = None,
    ):
        self.name = name
        self.source = source
        self.log_file = Path(log_file)
        self.socket_host = socket_host
        self.inject_port = inject_port
        self.forward_ports = list(forward_ports or [])
        self.verbose = verbose
        self.broadcaster = broadcaster
        self.session_clock = session_clock or SessionClock(TIMESTAMP_MODE_ABSOLUTE)
        self._clock = clock or (lambda: datetime.now().astimezone())

        self._queue: TrackedQueue = TrackedQueue(queue_maxsize)
        self._queue_maxsize = queue_maxsize
        self._stop = threading.Event()
        self._stream_clients: list = []
        self._clients_lock = threading.Lock()
        self._forward_clients: list = []
        self._forward_lock = threading.Lock()
        self._writer_thread: threading.Thread | None = None
        self._file_lock = threading.Lock()
        self._log_fd: TextIO | None = None
        self._inject_server: InjectServer | None = None
        self._forward_servers: list[ForwardServer] = []
    def start(self) -> None:
        self.log_file.parent.mkdir(parents=True, exist_ok=True)
        self._writer_thread = threading.Thread(
            target=self._writer_loop,
            daemon=True,
            name=f"{self.name}-writer",
        )
        self._writer_thread.start()
        try:
            try:
                self.source.start(self._on_source_line, self._stop, self.name)
            except OSError as exc:
                raise RuntimeError(f"[{self.name}] failed to start {type(self.source).__name__}: {exc}") from exc
            if self.inject_port:
                self._inject_server = InjectServer(
                    name=self.name,
                    host=self.socket_host,
                    port=self.inject_port,
                    stop=self._stop,
                    on_client_connect=self._add_stream_client,
                    on_client_disconnect=self._remove_stream_client,
                    on_json_line=self._ingest_json,
                )
                try:
                    self._inject_server.start()
                except OSError as exc:
                    raise RuntimeError(f"[{self.name}] failed to bind inject TCP {self.socket_host}:{self.inject_port}: {exc}") from exc
            self._forward_servers = []
            for port in self.forward_ports:
                server = ForwardServer(
                    name=self.name,
                    host=self.socket_host,
                    port=port,
                    stop=self._stop,
                    on_client_connect=self._add_forward_client,
                    on_client_disconnect=self._remove_forward_client,
                )
                self._forward_servers.append(server)
                try:
                    server.start()
                except OSError as exc:
                    raise RuntimeError(f"[{self.name}] failed to bind forward TCP {self.socket_host}:{port}: {exc}") from exc
        except Exception:
            self.stop()
            raise
        logging.info(
            "[%s] started  source=%s  inject=%s  forward=%s  log=%s",
            self.name,
            type(self.source).__name__,
            f":{self.inject_port}" if self.inject_port else "none",
            ",".join(f":{p}" for p in self.forward_ports) if self.forward_ports else "none",
            self.log_file,
        )

    def stop(self) -> None:
        self._stop.set()
        self.source.close()
        if self._inject_server is not None:
            self._inject_server.stop()
            self._inject_server = None
        for server in self._forward_servers:
            server.stop()
        self._forward_servers = []
        with self._clients_lock:
            for conn in list(self._stream_clients):
                try:
                    conn.close()
                except OSError:
                    pass
            self._stream_clients.clear()
        with self._forward_lock:
            for conn in list(self._forward_clients):
                try:
                    conn.close()
                except OSError:
                    pass
            self._forward_clients.clear()
        writer_thread = self._writer_thread
        self._writer_thread = None
        if writer_thread and writer_thread.is_alive():
            self._queue.put(None)
            writer_thread.join(timeout=2.0)

    def _on_source_line(self, message: str) -> None:
        self._queue.put(LogEntry(self._clock(), "SERIAL", message))

    def _format(self, entry: LogEntry) -> str:
        ts = self.session_clock.file_timestamp(entry.timestamp)
        is_serial = entry.source == "SERIAL"
        if self.verbose:
            line = f"[{ts}] [{self.name}] [{entry.source}] {entry.message}"
        elif is_serial:
            line = f"[{ts}] {entry.message}"
        else:
            line = f"[{ts}] [{entry.source}] {entry.message}"
        if entry.color and entry.color in ANSI:
            line = ANSI[entry.color] + line + ANSI["reset"]
        return line

    def _ws_payload(self, entry: LogEntry) -> dict:
        is_tx = entry.source.startswith("TX::")
        if entry.color and entry.color in ANSI:
            data = ANSI[entry.color] + entry.message + ANSI["reset"]
        else:
            data = entry.message
        return {
            "type": "tx" if is_tx else "rx",
            "data": data,
            "timestamp": self.session_clock.display_timestamp(entry.timestamp),
            "timestamp_iso": entry.timestamp.isoformat(timespec="milliseconds"),
            "timestamp_num": self.session_clock.numeric_timestamp(entry.timestamp),
            "source_id": self.name,
        }

    def _stream_payload(self, entry: LogEntry) -> bytes:
        payload = {
            "source_id": self.name,
            "source": entry.source,
            "message": entry.message,
            "timestamp": entry.timestamp.isoformat(timespec="milliseconds"),
        }
        if entry.color:
            payload["color"] = entry.color
        return json.dumps(payload).encode("utf-8") + b"\n"

    def _writer_loop(self) -> None:
        _flush_counter = 0
        while True:
            entry = self._queue.get()
            try:
                if entry is None:
                    with self._file_lock:
                        if self._log_fd is not None:
                            self._log_fd.flush()
                            os.fsync(self._log_fd.fileno())
                            self._log_fd.close()
                            self._log_fd = None
                    break
                # Log near-full warning periodically when queue is congested
                if self._queue_maxsize > 0:
                    stats = self._queue.stats()
                    if stats.utilization_pct >= 80:
                        if stats.near_full_events % 100 == 1:
                            logging.warning(
                                "[%s] queue congested: %d/%d (%.0f%%)  near_full=%d",
                                self.name,
                                stats.depth,
                                self._queue_maxsize,
                                stats.utilization_pct,
                                stats.near_full_events,
                            )
                line = self._format(entry)
                if self.verbose:
                    print(line, flush=True)
                with self._file_lock:
                    if self._log_fd is None:
                        self.log_file.parent.mkdir(parents=True, exist_ok=True)
                        self._log_fd = open(self.log_file, "a", encoding="utf-8")
                    self._log_fd.write(line + "\n")
                    _flush_counter += 1
                    if _flush_counter >= 100:
                        self._log_fd.flush()
                        _flush_counter = 0
                if self.broadcaster and not entry.no_ws:
                    self.broadcaster.broadcast(self._ws_payload(entry))
                self._stream_to_clients(self._stream_payload(entry))
                if entry.source == "SERIAL":
                    self._forward_to_clients((entry.message + "\n").encode("utf-8", errors="replace"))
            finally:
                self._queue.task_done()

    def wait_until_flushed(self) -> None:
        self._queue.join()
    def flush_log_file(self, *, locked: bool = False) -> None:
        if locked:
            if self._log_fd is not None:
                self._log_fd.flush()
                os.fsync(self._log_fd.fileno())
            return
        with self._file_lock:
            if self._log_fd is not None:
                self._log_fd.flush()
                os.fsync(self._log_fd.fileno())

    def lock_log_file(self) -> None:
        self._file_lock.acquire()

    def unlock_log_file(self) -> None:
        self._file_lock.release()

    def rotate_log_file(self, log_file: str, *, locked: bool = False) -> None:
        if locked:
            if self._log_fd is not None:
                self._log_fd.flush()
                os.fsync(self._log_fd.fileno())
                self._log_fd.close()
                self._log_fd = None
            self.log_file = Path(log_file)
            self.log_file.parent.mkdir(parents=True, exist_ok=True)
            self._log_fd = open(self.log_file, "a", encoding="utf-8")
            return
        with self._file_lock:
            if self._log_fd is not None:
                self._log_fd.flush()
                os.fsync(self._log_fd.fileno())
                self._log_fd.close()
                self._log_fd = None
            self.log_file = Path(log_file)
            self.log_file.parent.mkdir(parents=True, exist_ok=True)
            self._log_fd = open(self.log_file, "a", encoding="utf-8")
    def add_session_marker(self, message: str, *, no_ws: bool = True) -> None:
        self._queue.put(LogEntry(self._clock(), "SYSTEM", message, "cyan", no_ws=no_ws))

    def _stream_to_clients(self, data: bytes) -> None:
        with self._clients_lock:
            dead = []
            for conn in self._stream_clients:
                try:
                    conn.sendall(data)
                except OSError:
                    dead.append(conn)
            if dead:
                self._stream_clients = [c for c in self._stream_clients if c not in dead]

    def _forward_to_clients(self, data: bytes) -> None:
        with self._forward_lock:
            dead = []
            for conn in self._forward_clients:
                try:
                    conn.sendall(data)
                except OSError:
                    dead.append(conn)
            if dead:
                self._forward_clients = [c for c in self._forward_clients if c not in dead]

    def _add_stream_client(self, conn) -> None:
        with self._clients_lock:
            self._stream_clients.append(conn)

    def _remove_stream_client(self, conn) -> None:
        with self._clients_lock:
            try:
                self._stream_clients.remove(conn)
            except ValueError:
                pass

    def _add_forward_client(self, conn) -> None:
        with self._forward_lock:
            self._forward_clients.append(conn)

    def _remove_forward_client(self, conn) -> None:
        with self._forward_lock:
            try:
                self._forward_clients.remove(conn)
            except ValueError:
                pass

    def _write_source(self, data: bytes, source: str) -> None:
        self.source.write(data)
        printable = data.decode("utf-8", errors="replace").rstrip()
        self._queue.put(LogEntry(
            self._clock(),
            f"TX::{source}",
            printable,
            "yellow",
        ))

    def add_ui_clear_marker(self, scope: str = "pane") -> None:
        marker = f"[embed-log] UI clear ({scope})"
        self._queue.put(LogEntry(self._clock(), "SYSTEM", "", "cyan", no_ws=True))
        self._queue.put(LogEntry(self._clock(), "SYSTEM", marker, "cyan", no_ws=True))
        self._queue.put(LogEntry(self._clock(), "SYSTEM", "", "cyan", no_ws=True))
    def get_stats(self) -> dict:
        """Return per-source queue and throughput statistics."""
        qs = self._queue.stats()
        return {
            "name": self.name,
            "queue": dataclasses.asdict(qs),
            "source_type": type(self.source).__name__,
            "inject_port": self.inject_port,
            "forward_ports": list(self.forward_ports),
        }

    def _ingest_json(self, raw: bytes) -> None:
        try:
            msg = json.loads(raw.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError) as exc:
            logging.debug("bad message from client: %s", exc)
            return
        msg_type = msg.get("type", "log")
        source = msg.get("source", "TEST")
        if msg_type == "tx":
            data_str = msg.get("data", "")
            try:
                self._write_source(data_str.encode("utf-8"), source)
            except (serial.SerialException, TypeError) as exc:
                logging.warning("%s", exc)
        else:
            self._queue.put(LogEntry(
                self._clock(),
                source,
                str(msg.get("message", "")),
                msg.get("color"),
            ))


class LogServer:
    def __init__(
        self,
        sources: list,
        tabs: list,
        session_id: str,
        session_dir: str,
        logs_root: str,
        host: str = "127.0.0.1",
        verbose: bool = False,
        ws_port: int = 0,
        ws_ui: str = "frontend/index.html",
        config_path: str | None = None,
        job_id: str | None = None,
        open_browser: bool = False,
        app_name: str = "embed-log",
        theme_defaults: dict | None = None,
        source_labels: dict[str, str] | None = None,
        queue_maxsize: int = 20000,
        timestamp_mode: str = TIMESTAMP_MODE_ABSOLUTE,
    ):
        self._tabs = tabs
        self._session_id = session_id
        self._started_at = datetime.now().astimezone().isoformat(timespec="seconds")
        self._session_dir = Path(session_dir)
        self._logs_root = Path(logs_root)
        self._job_id = job_id
        self._app_name = app_name
        self._theme_defaults = theme_defaults or {}
        self._source_labels = source_labels or {s["name"]: s.get("label", s["name"]) for s in sources}
        self._queue_maxsize = queue_maxsize
        self._timestamp_mode = timestamp_mode if timestamp_mode in TIMESTAMP_MODES else TIMESTAMP_MODE_ABSOLUTE
        self._rotate_lock = threading.Lock()
        self._export_lock = threading.Lock()
        self._session_lock = threading.Lock()
        self._session_clock = SessionClock(self._timestamp_mode)

        self._source_files = {s["name"]: str(s["log_file"]) for s in sources}
        self._session = SessionManager(
            session_id=self._session_id,
            session_dir=self._session_dir,
            tabs=self._tabs,
            source_files=self._source_files,
            source_labels=self._source_labels,
            started_at=self._started_at,
            config_path=config_path,
            job_id=self._job_id,
            app_name=self._app_name,
            timestamp_mode=self._timestamp_mode,
            first_log_at=self._session_clock.first_log_at(),
        )
        self._session_info = self._session.build_session_info()
        existing_markers = self._session.load_markers()
        if existing_markers:
            self._session_info["markers"] = existing_markers
        self._session_clock = SessionClock(
            self._timestamp_mode,
            on_origin_set=self._handle_first_log_at,
        )
        self._session.set_first_log_at(self._session_clock.first_log_at())
        self._exporter = SessionExporter(
            session_html_path=self._session.html_path,
            source_files=self._source_files,
            tabs=self._tabs,
            source_labels=self._source_labels,
            timestamp_mode=self._timestamp_mode,
            first_log_at=self._session.first_log_at,
        )

        broadcaster: WebSocketBroadcaster | None = None
        if ws_port:
            broadcaster = WebSocketBroadcaster(
                ws_ui,
                host,
                ws_port,
                tabs,
                session_info=dict(self._session_info),
                sessions_root=str(self._logs_root),
                on_all_clients_disconnected=lambda: self.export_session_html("last_ws_disconnect"),
                on_export_session_html=lambda: self.export_session_html("manual_ui"),
                on_rotate_session=lambda: self.rotate_session("manual_ui"),
                on_save_snippet=lambda text, panes, scope, label: self._session.save_snippet(text, panes=panes, scope=scope, label=label),
                on_save_markers=lambda markers: self._session.save_markers(markers),
                open_browser=open_browser,
                app_name=app_name,
                theme_defaults=self._theme_defaults,
                source_labels=self._source_labels,
            )

        self._broadcaster = broadcaster
        self._managers = [
            SourceManager(
                name=s["name"],
                source=s["source"],
                log_file=s["log_file"],
                socket_host=host,
                inject_port=s.get("inject_port"),
                forward_ports=s.get("forward_ports", []),
                verbose=verbose,
                broadcaster=broadcaster,
                queue_maxsize=self._queue_maxsize,
                session_clock=self._session_clock,
            )
            for s in sources
        ]

        if broadcaster:
            for mgr in self._managers:
                broadcaster.register_source(mgr.name, mgr)

        self._session.write_manifest(
            reason="start",
            exported_html=self._session_info.get("html_ready", False),
            html_status=self._session_info.get("html_status", "pending"),
            html_updated_at=self._session_info.get("html_updated_at"),
            html_error=self._session_info.get("html_error"),
        )

    def _update_session_info(self, updates: dict) -> None:
        with self._session_lock:
            self._session_info.update(updates)

    def _handle_first_log_at(self, first_log_at: str) -> None:
        self._session.set_first_log_at(first_log_at)
        self._update_session_info({"first_log_at": first_log_at})
        self._exporter.set_first_log_at(first_log_at)
        with self._session_lock:
            exported_html = self._session_info.get("html_ready", False)
            html_status = self._session_info.get("html_status", "pending")
            html_updated_at = self._session_info.get("html_updated_at")
            html_error = self._session_info.get("html_error")
        self._session.write_manifest(
            reason="first_log_at",
            exported_html=exported_html,
            html_status=html_status,
            html_updated_at=html_updated_at,
            html_error=html_error,
        )
        if self._broadcaster:
            with self._session_lock:
                session_snapshot = dict(self._session_info)
            self._broadcaster.update_session_info({"first_log_at": first_log_at})
            self._broadcaster.broadcast({
                "type": "session_info",
                "session": session_snapshot,
            })
    def _publish_html_state(self) -> None:
        if not self._broadcaster:
            return
        with self._session_lock:
            updates = {
                "html_ready": self._session_info.get("html_ready", False),
                "html_status": self._session_info.get("html_status", "pending"),
                "html_updated_at": self._session_info.get("html_updated_at"),
                "html_error": self._session_info.get("html_error"),
                "last_export_reason": self._session_info.get("last_export_reason"),
                "first_log_at": self._session_info.get("first_log_at"),
            }
        self._broadcaster.update_session_info(updates)
        self._broadcaster.broadcast({
            "type": "session_html_status",
            "session_id": self._session_id,
            **updates,
        })
    def _build_source_files_for_session(self, session_id: str, session_dir: Path) -> dict[str, str]:
        tab_label_by_source: dict[str, str] = {}
        for tab in self._tabs:
            for pane in tab.get("panes", []):
                tab_label_by_source[pane] = tab.get("label", "session")
        files = {}
        for mgr in self._managers:
            tab_label = tab_label_by_source.get(mgr.name, "session")
            log_name = f"{slugify(tab_label)}__{slugify(mgr.name)}__{session_id}.log"
            files[mgr.name] = str(session_dir / log_name)
        return files

    def _new_session_id_and_dir(self) -> tuple[str, Path]:
        base_session_id = datetime.now().astimezone().strftime("%Y-%m-%d_%H-%M-%S")
        if self._job_id:
            base_session_id = f"{base_session_id}__{slugify(self._job_id)}"
        session_id = base_session_id
        session_dir = self._logs_root / session_id
        i = 1
        while session_dir.exists():
            session_id = f"{base_session_id}_{i}"
            session_dir = self._logs_root / session_id
            i += 1
        session_dir.mkdir(parents=True, exist_ok=True)
        return session_id, session_dir

    def export_session_html(self, reason: str, *, log_files_locked: bool = False) -> bool:
        with self._export_lock:
            with self._session_lock:
                if self._session_info.get("html_status") == "updating":
                    return False

            for mgr in self._managers:
                mgr.wait_until_flushed()
            for mgr in self._managers:
                mgr.flush_log_file(locked=log_files_locked)

            self._update_session_info({
                "html_status": "updating",
                "html_error": None,
                "last_export_reason": reason,
            })
            self._publish_html_state()

            ok = self._exporter.export_html(reason)
            if ok:
                updated_at = datetime.now().astimezone().isoformat(timespec="seconds")
                self._session.write_manifest(
                    reason=reason,
                    exported_html=True,
                    html_status="ready",
                    html_updated_at=updated_at,
                    html_error=None,
                )
                self._update_session_info({
                    "html_ready": True,
                    "html_status": "ready",
                    "html_updated_at": updated_at,
                    "html_error": None,
                    "last_export_reason": reason,
                })
                self._publish_html_state()
                return True

            err = "export failed"
            with self._session_lock:
                html_updated_at = self._session_info.get("html_updated_at")
            self._session.write_manifest(
                reason=reason,
                exported_html=self._session.html_path.is_file(),
                html_status="error",
                html_updated_at=html_updated_at,
                html_error=err,
            )
            self._update_session_info({
                "html_ready": self._session.html_path.is_file(),
                "html_status": "error",
                "html_error": err,
                "last_export_reason": reason,
            })
            self._publish_html_state()
            return False

    def rotate_session(self, reason: str = "manual_ui") -> dict:
        with self._rotate_lock:
            with self._session_lock:
                old_info = dict(self._session_info)
            close_msg = f"[embed-log] session closed: {reason}"
            for mgr in self._managers:
                mgr.add_session_marker(close_msg, no_ws=False)
            for mgr in self._managers:
                mgr.wait_until_flushed()

            locked: list[SourceManager] = []
            try:
                for mgr in self._managers:
                    mgr.lock_log_file()
                    locked.append(mgr)

                self.export_session_html(f"rotate:{reason}", log_files_locked=True)

                session_id, session_dir = self._new_session_id_and_dir()
                started_at = datetime.now().astimezone().isoformat(timespec="seconds")
                source_files = self._build_source_files_for_session(session_id, session_dir)

                for mgr in self._managers:
                    mgr.rotate_log_file(source_files[mgr.name], locked=True)
            finally:
                for mgr in reversed(locked):
                    mgr.unlock_log_file()

            self._session_id = session_id
            self._started_at = started_at
            self._session_dir = session_dir
            self._source_files = source_files
            self._session_clock.reset()
            self._session = SessionManager(
                session_id=self._session_id,
                session_dir=self._session_dir,
                tabs=self._tabs,
                source_files=self._source_files,
                source_labels=self._source_labels,
                started_at=self._started_at,
                config_path=self._session.config_path if hasattr(self._session, "config_path") else None,
                job_id=self._job_id,
                app_name=self._app_name,
                timestamp_mode=self._timestamp_mode,
                first_log_at=self._session_clock.first_log_at(),
            )
            self._exporter = SessionExporter(
                session_html_path=self._session.html_path,
                source_files=self._source_files,
                tabs=self._tabs,
                source_labels=self._source_labels,
                timestamp_mode=self._timestamp_mode,
                first_log_at=self._session.first_log_at,
            )
            self._session_info = self._session.build_session_info()
            self._session.write_manifest(
                reason="rotate_start",
                exported_html=False,
                html_status="pending",
                html_updated_at=None,
                html_error=None,
            )

            if self._broadcaster:
                with self._session_lock:
                    session_snapshot = dict(self._session_info)
                self._broadcaster.update_session_info(session_snapshot)
                self._broadcaster.broadcast({
                    "type": "session_rotated",
                    "old_session": old_info,
                    "session": session_snapshot,
                })

            start_msg = f"[embed-log] clean session started: {reason}"
            for mgr in self._managers:
                mgr.add_session_marker(start_msg, no_ws=False)
            with self._session_lock:
                return {"old_session": old_info, "session": self._session_info}

    def start(self) -> None:
        if self._broadcaster:
            self._broadcaster.start()
        try:
            for mgr in self._managers:
                mgr.start()
        except Exception:
            self.stop()
            raise

    def stop(self) -> None:
        for mgr in self._managers:
            mgr.stop()
        if self._broadcaster:
            self._broadcaster.stop()

    def run_forever(self) -> None:
        logging.info("session timezone: %s", datetime.now().astimezone().tzname())
        self.start()
        stop_event = threading.Event()

        def _handler(sig, frame):
            logging.info("shutting down…")
            self.stop()
            self.export_session_html("signal")
            stop_event.set()

        signal.signal(signal.SIGINT, _handler)
        signal.signal(signal.SIGTERM, _handler)
        logging.info("log server running — press Ctrl-C to stop")
        stop_event.wait()
