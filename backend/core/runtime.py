from __future__ import annotations

import json
import logging
import queue
import signal
import threading
from datetime import datetime
from pathlib import Path
from typing import Optional

import serial

from ..net import ForwardServer, InjectServer, WebSocketBroadcaster
from ..session import SessionExporter, SessionManager
from ..sources import LogSource
from .naming import slugify

# ---------------------------------------------------------------------------
# ANSI colors available to clients
# ---------------------------------------------------------------------------
ANSI = {
    "red":     "\033[31m",
    "green":   "\033[32m",
    "yellow":  "\033[33m",
    "blue":    "\033[34m",
    "magenta": "\033[35m",
    "cyan":    "\033[36m",
    "white":   "\033[37m",
    "bold":    "\033[1m",
    "reset":   "\033[0m",
}


def _slug(value: str) -> str:
    return slugify(value)


class LogEntry:
    __slots__ = ("timestamp", "source", "message", "color")

    def __init__(self, timestamp: datetime, source: str, message: str,
                 color: Optional[str] = None):
        self.timestamp = timestamp
        self.source = source
        self.message = message
        self.color = color


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
        inject_port: Optional[int] = None,
        forward_ports: Optional[list[int]] = None,
        verbose: bool = False,
        broadcaster: Optional[WebSocketBroadcaster] = None,
    ):
        self.name = name
        self.source = source
        self.log_file = Path(log_file)
        self.socket_host = socket_host
        self.inject_port = inject_port
        self.forward_ports = list(forward_ports or [])
        self.verbose = verbose
        self.broadcaster = broadcaster

        self._queue: queue.Queue[Optional[LogEntry]] = queue.Queue()
        self._stop = threading.Event()
        self._stream_clients: list = []
        self._clients_lock = threading.Lock()
        self._forward_clients: list = []
        self._forward_lock = threading.Lock()
        self._writer_thread: Optional[threading.Thread] = None
        self._inject_server: Optional[InjectServer] = None
        self._forward_servers: list[ForwardServer] = []

    def start(self) -> None:
        self.log_file.parent.mkdir(parents=True, exist_ok=True)
        self._writer_thread = threading.Thread(
            target=self._writer_loop,
            daemon=True,
            name=f"{self.name}-writer",
        )
        self._writer_thread.start()
        self.source.start(self._on_source_line, self._stop, self.name)
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
            self._inject_server.start()
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
            server.start()
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
        self._queue.put(None)
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
        if self._writer_thread and self._writer_thread.is_alive():
            self._writer_thread.join(timeout=2.0)

    def _on_source_line(self, message: str) -> None:
        self._queue.put(LogEntry(datetime.now().astimezone(), "SERIAL", message))

    def _format(self, entry: LogEntry) -> str:
        ts = entry.timestamp.isoformat(timespec="milliseconds")
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
        ts = entry.timestamp.strftime("%m-%d %H:%M:%S.%f")[:-3]
        return {
            "type": "tx" if is_tx else "rx",
            "data": data,
            "timestamp": ts,
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
        with open(self.log_file, "a", encoding="utf-8") as f:
            while True:
                entry = self._queue.get()
                if entry is None:
                    break
                line = self._format(entry)
                if self.verbose:
                    print(line, flush=True)
                f.write(line + "\n")
                f.flush()
                if self.broadcaster:
                    self.broadcaster.broadcast(self._ws_payload(entry))
                self._stream_to_clients(self._stream_payload(entry))
                if entry.source == "SERIAL":
                    self._forward_to_clients((entry.message + "\n").encode("utf-8", errors="replace"))

    def _stream_to_clients(self, data: bytes) -> None:
        with self._clients_lock:
            dead = []
            for conn in self._stream_clients:
                try:
                    conn.sendall(data)
                except OSError:
                    dead.append(conn)
            for conn in dead:
                self._stream_clients.remove(conn)

    def _forward_to_clients(self, data: bytes) -> None:
        with self._forward_lock:
            dead = []
            for conn in self._forward_clients:
                try:
                    conn.sendall(data)
                except OSError:
                    dead.append(conn)
            for conn in dead:
                try:
                    self._forward_clients.remove(conn)
                except ValueError:
                    pass

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
            datetime.now().astimezone(),
            f"TX::{source}",
            printable,
            "yellow",
        ))

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
                datetime.now().astimezone(),
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
        config_path: Optional[str] = None,
        job_id: Optional[str] = None,
        open_browser: bool = False,
        app_name: str = "embed-log",
    ):
        self._tabs = tabs
        self._session_id = session_id
        self._started_at = datetime.now().astimezone().isoformat(timespec="seconds")
        self._session_dir = Path(session_dir)
        self._logs_root = Path(logs_root)
        self._job_id = job_id
        self._app_name = app_name

        self._source_files = {s["name"]: str(s["log_file"]) for s in sources}
        self._session = SessionManager(
            session_id=self._session_id,
            session_dir=self._session_dir,
            tabs=self._tabs,
            source_files=self._source_files,
            started_at=self._started_at,
            config_path=config_path,
            job_id=self._job_id,
            app_name=self._app_name,
        )
        self._exporter = SessionExporter(
            session_html_path=self._session.html_path,
            source_files=self._source_files,
            tabs=self._tabs,
        )
        self._session_info = self._session.build_session_info()
        self._export_lock = threading.Lock()

        broadcaster: Optional[WebSocketBroadcaster] = None
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
                open_browser=open_browser,
                app_name=app_name,
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

    def _publish_html_state(self) -> None:
        if not self._broadcaster:
            return
        updates = {
            "html_ready": self._session_info.get("html_ready", False),
            "html_status": self._session_info.get("html_status", "pending"),
            "html_updated_at": self._session_info.get("html_updated_at"),
            "html_error": self._session_info.get("html_error"),
            "last_export_reason": self._session_info.get("last_export_reason"),
        }
        self._broadcaster.update_session_info(updates)
        self._broadcaster.broadcast({
            "type": "session_html_status",
            "session_id": self._session_id,
            **updates,
        })

    def export_session_html(self, reason: str) -> bool:
        with self._export_lock:
            if self._session_info.get("html_status") == "updating":
                return False

            self._session_info.update({
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
                self._session_info.update({
                    "html_ready": True,
                    "html_status": "ready",
                    "html_updated_at": updated_at,
                    "html_error": None,
                    "last_export_reason": reason,
                })
                self._publish_html_state()
                return True

            err = "export failed"
            self._session.write_manifest(
                reason=reason,
                exported_html=self._session.html_path.is_file(),
                html_status="error",
                html_updated_at=self._session_info.get("html_updated_at"),
                html_error=err,
            )
            self._session_info.update({
                "html_ready": self._session.html_path.is_file(),
                "html_status": "error",
                "html_error": err,
                "last_export_reason": reason,
            })
            self._publish_html_state()
            return False

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
