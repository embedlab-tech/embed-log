"""
embed-log — log aggregator with WebSocket UI and TCP inject port.

Usage:
    python3 server.py
        --source DEVICE_A uart:/dev/ttyUSB0
        --source DEVICE_B uart:/dev/ttyUSB1
        --inject DEVICE_A 5001
        --inject DEVICE_B 5002
        --tab "Devices" DEVICE_A DEVICE_B
        --ws-port 8080
"""

import argparse
import asyncio
import json
import logging
import queue
import re
import select
import signal
import socket
import subprocess
import sys
import threading
from abc import ABC, abstractmethod
from datetime import datetime
from pathlib import Path
from typing import Callable, Optional

import aiohttp
from aiohttp import web
import serial

_config_import_error: Optional[Exception] = None
try:
    from .config import ConfigError, load_config
except Exception as exc1:
    try:
        from backend.config import ConfigError, load_config
    except Exception:
        try:
            from config import ConfigError, load_config
        except Exception as exc3:
            _config_import_error = exc3

            class ConfigError(ValueError):
                pass

            def load_config(path):
                raise ConfigError(
                    "YAML config support unavailable. Install dependencies with: pip install -r requirements.txt"
                ) from _config_import_error

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
    return re.sub(r"[^a-zA-Z0-9]+", "-", value).strip("-") or "x"


class LogEntry:
    __slots__ = ("timestamp", "source", "message", "color")

    def __init__(self, timestamp: datetime, source: str, message: str,
                 color: Optional[str] = None):
        self.timestamp = timestamp
        self.source = source
        self.message = message
        self.color = color


# ---------------------------------------------------------------------------
# Log sources
# ---------------------------------------------------------------------------

class LogSource(ABC):
    """
    Abstract base for anything that produces log lines.
    Subclasses implement start(); only UartSource implements write().
    """

    @abstractmethod
    def start(self, on_line: Callable[[str], None],
              stop: threading.Event, name: str) -> None:
        """Start reading in a background thread. on_line(text) per line."""

    def write(self, data: bytes) -> None:
        raise TypeError(f"{type(self).__name__} does not support write")

    @property
    def supports_write(self) -> bool:
        return False


class UartSource(LogSource):
    def __init__(self, port: str, baudrate: int = 115200):
        self.port = port
        self.baudrate = baudrate
        self._ser: Optional[serial.Serial] = None
        self._ser_lock = threading.Lock()

    @property
    def supports_write(self) -> bool:
        return True

    def write(self, data: bytes) -> None:
        with self._ser_lock:
            if self._ser is None or not self._ser.is_open:
                raise serial.SerialException("serial port not open — cannot send TX data")
            self._ser.write(data)

    def start(self, on_line, stop, name):
        threading.Thread(
            target=self._run, args=(on_line, stop, name),
            daemon=True, name=f"{name}-uart",
        ).start()

    def _run(self, on_line, stop, name):
        while not stop.is_set():
            try:
                with serial.Serial(self.port, self.baudrate, timeout=1) as ser:
                    logging.info("[%s] opened serial %s @ %d", name, self.port, self.baudrate)
                    with self._ser_lock:
                        self._ser = ser
                    try:
                        while not stop.is_set():
                            raw = ser.readline()
                            if raw:
                                on_line(raw.decode("utf-8", errors="replace").rstrip())
                    finally:
                        with self._ser_lock:
                            self._ser = None
            except serial.SerialException as exc:
                logging.warning("[%s] serial error: %s — retrying in 3 s", name, exc)
                stop.wait(3)


class UdpSource(LogSource):
    """Listens for UDP datagrams; each datagram may contain multiple newline-separated lines."""

    def __init__(self, port: int):
        self.port = port

    def start(self, on_line, stop, name):
        threading.Thread(
            target=self._run, args=(on_line, stop, name),
            daemon=True, name=f"{name}-udp",
        ).start()

    def _run(self, on_line, stop, name):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.bind(("0.0.0.0", self.port))
            sock.settimeout(1.0)
            logging.info("[%s] listening on UDP :%d", name, self.port)
            while not stop.is_set():
                try:
                    data, _ = sock.recvfrom(65535)
                    for line in data.decode("utf-8", errors="replace").splitlines():
                        if line.strip():
                            on_line(line.rstrip())
                except socket.timeout:
                    continue


# ---------------------------------------------------------------------------
# WebSocket broadcaster
# ---------------------------------------------------------------------------

class WebSocketBroadcaster:
    """
    aiohttp server in a background thread.

    GET /    → serves the UI HTML file
    GET /ws  → WebSocket; broadcasts log entries, accepts send_raw commands.

    On every new WS connection sends a "config" message so the browser
    knows the tab/pane layout upfront.
    """

    def __init__(
        self,
        html_path: str,
        host: str,
        port: int,
        tabs: list,
        session_info: Optional[dict] = None,
        sessions_root: Optional[str] = None,
        on_all_clients_disconnected: Optional[Callable[[], None]] = None,
    ):
        self._html_path = Path(html_path)
        self._host = host
        self._port = port
        self._tabs = tabs          # [{"label": str, "panes": [str, ...]}]
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._clients: set = set()
        self._source_map: dict = {}   # name → SourceManager
        self._session_info = session_info or {}
        self._sessions_root = Path(sessions_root) if sessions_root else None
        self._on_all_clients_disconnected = on_all_clients_disconnected
        self._no_clients_handle = None
        self._thread: Optional[threading.Thread] = None
        self._started = threading.Event()
        self._start_error: Optional[Exception] = None
        self._stop_async: Optional[asyncio.Event] = None

    def register_source(self, name: str, mgr) -> None:
        self._source_map[name] = mgr

    def update_session_info(self, updates: dict) -> None:
        self._session_info.update(updates)

    def broadcast(self, msg: dict) -> None:
        if self._loop and not self._loop.is_closed() and self._clients:
            asyncio.run_coroutine_threadsafe(self._broadcast_async(msg), self._loop)

    async def _broadcast_async(self, msg: dict) -> None:
        if not self._clients:
            return
        data = json.dumps(msg)
        dead = set()
        for ws in list(self._clients):
            try:
                await ws.send_str(data)
            except Exception:
                dead.add(ws)
        self._clients -= dead

    def start(self) -> None:
        self._thread = threading.Thread(target=self._run, daemon=True, name="ws-broadcaster")
        self._thread.start()
        self._started.wait(timeout=5.0)
        if self._start_error is not None:
            raise RuntimeError(f"failed to start WebSocket UI: {self._start_error}")

    def stop(self) -> None:
        if self._loop and not self._loop.is_closed() and self._stop_async is not None:
            self._loop.call_soon_threadsafe(self._stop_async.set)
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=3.0)

    def _run(self) -> None:
        self._loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self._loop)
        try:
            self._loop.run_until_complete(self._serve())
        except Exception as exc:
            self._start_error = exc
            self._started.set()
            logging.warning("WebSocket UI failed: %s", exc)
        finally:
            try:
                self._loop.close()
            except Exception:
                pass

    async def _serve(self) -> None:
        app = web.Application()
        app.router.add_get("/ws", self._ws_handler)
        app.router.add_get("/api/session/current", self._session_current_handler)
        app.router.add_get("/api/sessions", self._sessions_list_handler)
        app.router.add_get("/sessions/{session_id}/{filename}", self._session_file_handler)
        app.router.add_get("/", self._index_handler)
        app.router.add_get("/{filename}", self._static_handler)
        runner = web.AppRunner(app)
        await runner.setup()
        site = web.TCPSite(runner, self._host, self._port)
        await site.start()
        self._stop_async = asyncio.Event()
        self._started.set()
        logging.info("UI ready at http://%s:%d/  (WebSocket: ws://%s:%d/ws)",
                     self._host, self._port, self._host, self._port)
        await self._stop_async.wait()
        await runner.cleanup()

    async def _index_handler(self, request: web.Request) -> web.Response:
        if not self._html_path.exists():
            raise web.HTTPNotFound(reason=f"UI file not found: {self._html_path}")
        return web.FileResponse(self._html_path)

    async def _static_handler(self, request: web.Request) -> web.Response:
        filename = request.match_info["filename"]
        if "/" in filename or ".." in filename:
            raise web.HTTPForbidden()
        path = self._html_path.parent / filename
        if not path.is_file():
            raise web.HTTPNotFound()
        return web.FileResponse(path)

    async def _session_current_handler(self, request: web.Request) -> web.Response:
        return web.json_response(self._session_info)

    async def _sessions_list_handler(self, request: web.Request) -> web.Response:
        if self._sessions_root is None or not self._sessions_root.is_dir():
            return web.json_response({"sessions": [], "current": self._session_info.get("id")})

        current = self._session_info.get("id")
        sessions = []
        for child in sorted(self._sessions_root.iterdir(), reverse=True):
            if not child.is_dir():
                continue
            session_id = child.name
            manifest_path = child / "manifest.json"
            html_path = child / "session.html"

            started_at = None
            tabs = []
            if manifest_path.is_file():
                try:
                    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                    started_at = manifest.get("started_at")
                    tabs = manifest.get("tabs") or []
                except Exception:
                    pass

            sessions.append({
                "id": session_id,
                "started_at": started_at,
                "html_ready": html_path.is_file(),
                "html": f"/sessions/{session_id}/session.html",
                "manifest": f"/sessions/{session_id}/manifest.json",
                "tabs": tabs,
            })

        return web.json_response({"sessions": sessions, "current": current})

    async def _session_file_handler(self, request: web.Request) -> web.Response:
        if self._sessions_root is None:
            raise web.HTTPNotFound()
        session_id = request.match_info["session_id"]
        filename = request.match_info["filename"]
        if any(x in session_id for x in ["..", "/"]) or any(x in filename for x in ["..", "/"]):
            raise web.HTTPForbidden()
        path = self._sessions_root / session_id / filename
        if not path.is_file():
            raise web.HTTPNotFound()
        return web.FileResponse(path)

    async def _ws_handler(self, request: web.Request) -> web.WebSocketResponse:
        ws = web.WebSocketResponse()
        await ws.prepare(request)
        logging.info("WS client connected: %s", request.remote)

        # Send tab layout BEFORE adding to the broadcast set so that the config
        # message is always the first thing the browser receives — no log entries
        # can arrive before it and trigger premature dynamic tab creation.
        await ws.send_str(json.dumps({
            "type": "config",
            "tabs": self._tabs,
            "session": self._session_info,
        }))
        self._clients.add(ws)
        if self._no_clients_handle is not None:
            self._no_clients_handle.cancel()
            self._no_clients_handle = None

        try:
            async for msg in ws:
                if msg.type == aiohttp.WSMsgType.TEXT:
                    try:
                        await self._handle_command(json.loads(msg.data))
                    except Exception:
                        pass
                elif msg.type == aiohttp.WSMsgType.ERROR:
                    logging.debug("WS error: %s", ws.exception())
        finally:
            self._clients.discard(ws)
            if not self._clients:
                self._schedule_no_clients_callback()
            logging.info("WS client disconnected: %s", request.remote)
        return ws

    def _schedule_no_clients_callback(self) -> None:
        if self._on_all_clients_disconnected is None or self._loop is None:
            return
        if self._no_clients_handle is not None:
            self._no_clients_handle.cancel()
        self._no_clients_handle = self._loop.call_later(1.0, self._fire_no_clients_callback)

    def _fire_no_clients_callback(self) -> None:
        self._no_clients_handle = None
        if self._on_all_clients_disconnected is None or self._clients:
            return
        threading.Thread(target=self._on_all_clients_disconnected, daemon=True).start()

    async def _handle_command(self, msg: dict) -> None:
        if msg.get("cmd") != "send_raw":
            return
        name = msg.get("id", "")
        data = msg.get("data", "")
        mgr = self._source_map.get(name)
        if mgr:
            try:
                mgr._write_source(data.encode("utf-8"), source="UI")
            except (serial.SerialException, TypeError) as exc:
                logging.warning("send_raw failed for '%s': %s", name, exc)


# ---------------------------------------------------------------------------
# Source manager — one per named source
# ---------------------------------------------------------------------------

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
            threading.Thread(target=self._inject_loop, daemon=True,
                             name=f"{self.name}-inject").start()
        for port in self.forward_ports:
            threading.Thread(
                target=self._forward_loop,
                args=(port,),
                daemon=True,
                name=f"{self.name}-fwd-{port}",
            ).start()
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

    # ------------------------------------------------------------------
    # Formatting
    # ------------------------------------------------------------------

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

    # ------------------------------------------------------------------
    # Writer thread — single consumer of the queue
    # ------------------------------------------------------------------

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

    # ------------------------------------------------------------------
    # Serial TX
    # ------------------------------------------------------------------

    def _write_source(self, data: bytes, source: str) -> None:
        self.source.write(data)
        printable = data.decode("utf-8", errors="replace").rstrip()
        self._queue.put(LogEntry(
            datetime.now().astimezone(),
            f"TX::{source}",
            printable,
            "yellow",
        ))

    # ------------------------------------------------------------------
    # Inject TCP server
    # ------------------------------------------------------------------

    def _inject_loop(self) -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as srv:
            srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            srv.bind((self.socket_host, self.inject_port))
            srv.listen(16)
            srv.settimeout(1.0)
            while not self._stop.is_set():
                try:
                    conn, addr = srv.accept()
                except socket.timeout:
                    continue
                with self._clients_lock:
                    self._stream_clients.append(conn)
                threading.Thread(
                    target=self._handle_inject_client,
                    args=(conn, addr),
                    daemon=True,
                    name=f"{self.name}-client-{addr[1]}",
                ).start()

    def _handle_inject_client(self, conn: socket.socket, addr) -> None:
        buf = b""
        try:
            while not self._stop.is_set():
                try:
                    ready, _, _ = select.select([conn], [], [], 1.0)
                except OSError:
                    break
                if not ready:
                    continue
                try:
                    chunk = conn.recv(4096)
                except OSError:
                    break
                if not chunk:
                    break
                buf += chunk
                while b"\n" in buf:
                    raw_line, buf = buf.split(b"\n", 1)
                    self._ingest_json(raw_line)
        finally:
            with self._clients_lock:
                try:
                    self._stream_clients.remove(conn)
                except ValueError:
                    pass
            try:
                conn.close()
            except OSError:
                pass

    def _forward_loop(self, forward_port: int) -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as srv:
            srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            srv.bind((self.socket_host, forward_port))
            srv.listen(16)
            srv.settimeout(1.0)
            while not self._stop.is_set():
                try:
                    conn, addr = srv.accept()
                except socket.timeout:
                    continue
                with self._forward_lock:
                    self._forward_clients.append(conn)
                threading.Thread(
                    target=self._handle_forward_client,
                    args=(conn, addr),
                    daemon=True,
                    name=f"{self.name}-fwd-client-{addr[1]}",
                ).start()

    def _handle_forward_client(self, conn: socket.socket, addr) -> None:
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
            with self._forward_lock:
                try:
                    self._forward_clients.remove(conn)
                except ValueError:
                    pass
            try:
                conn.close()
            except OSError:
                pass

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


# ---------------------------------------------------------------------------
# Top-level server
# ---------------------------------------------------------------------------

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
    ):
        self._tabs = tabs
        self._session_id = session_id
        self._started_at = datetime.now().astimezone().isoformat(timespec="seconds")
        self._session_dir = Path(session_dir)
        self._logs_root = Path(logs_root)
        self._manifest_path = self._session_dir / "manifest.json"
        self._html_path = self._session_dir / "session.html"
        self._config_path = config_path
        self._export_lock = threading.Lock()

        self._source_files = {s["name"]: str(s["log_file"]) for s in sources}
        self._session_info = {
            "id": self._session_id,
            "dir": str(self._session_dir),
            "manifest": f"/sessions/{self._session_id}/manifest.json",
            "html": f"/sessions/{self._session_id}/session.html",
            "html_ready": False,
            "api": "/api/session/current",
            "tabs": self._tabs,
            "sources": [
                {"name": name, "log": f"/sessions/{self._session_id}/{Path(path).name}"}
                for name, path in self._source_files.items()
            ],
        }

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

        self._write_manifest(reason="start")

    def _write_manifest(self, *, reason: str, exported_html: bool = False) -> None:
        manifest = {
            "session_id": self._session_id,
            "session_dir": str(self._session_dir),
            "started_at": self._started_at,
            "config_path": self._config_path,
            "tabs": self._tabs,
            "source_files": self._source_files,
            "session_html": str(self._html_path) if exported_html else None,
            "last_export_reason": reason if exported_html else None,
        }
        self._manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    def export_session_html(self, reason: str) -> None:
        with self._export_lock:
            script = Path(__file__).resolve().parents[1] / "utils" / "merge_logs.py"
            if not script.is_file():
                return

            tabs_for_export = self._tabs or [
                {"label": name, "panes": [name]} for name in self._source_files.keys()
            ]

            cmd = [sys.executable, str(script)]
            for tab in tabs_for_export:
                cmd.extend(["--tab", tab["label"]])
                for pane in tab.get("panes", []):
                    file_path = self._source_files.get(pane)
                    if not file_path:
                        continue
                    cmd.extend([pane, file_path])
            cmd.extend(["--output", str(self._html_path)])

            try:
                proc = subprocess.run(cmd, capture_output=True, text=True)
                if proc.returncode != 0:
                    logging.warning("session export failed (%s): %s", reason, proc.stderr.strip())
                    return
            except Exception as exc:
                logging.warning("session export failed (%s): %s", reason, exc)
                return

            self._write_manifest(reason=reason, exported_html=True)
            self._session_info["html_ready"] = True
            if self._broadcaster:
                self._broadcaster.update_session_info({"html_ready": True})

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


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

_SOURCE_RE = re.compile(r"^(uart|udp):(.+)$", re.IGNORECASE)
_UART_BAUD_RE = re.compile(r"^(.+)@(\d+)$")


def _parse_source(name: str, spec: str, default_baudrate: int) -> LogSource:
    m = _SOURCE_RE.match(spec)
    if not m:
        raise argparse.ArgumentTypeError(
            f"--source {name!r}: invalid spec {spec!r}. "
            f"Use uart:/dev/path[@baud] or udp:PORT"
        )
    kind, arg = m.group(1).lower(), m.group(2)

    if kind == "uart":
        bm = _UART_BAUD_RE.match(arg)
        if bm:
            return UartSource(bm.group(1), int(bm.group(2)))
        return UartSource(arg, default_baudrate)

    # udp
    try:
        return UdpSource(int(arg))
    except ValueError:
        raise argparse.ArgumentTypeError(
            f"--source {name!r}: udp port must be an integer, got {arg!r}"
        )


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="embed-log — log aggregator with WebSocket UI and TCP inject port.",
        epilog=(
            "Examples:\n"
            "  embed-log run --config embed-log.yml\n"
            "  python backend/server.py --config embed-log.yml\n"
            "  python backend/server.py --source DEVICE_A uart:/dev/ttyUSB0 --inject DEVICE_A 5001"
        ),
    )
    parser.add_argument(
        "--config", "-c", metavar="FILE", default=None,
        help="YAML config file (version: 1). CLI flags override config values.",
    )
    parser.add_argument(
        "--source", nargs=2, action="append", metavar=("NAME", "TYPE"),
        dest="sources", default=[],
        help="NAME  uart:/dev/path[@baud] | udp:PORT  — repeat for multiple sources",
    )
    parser.add_argument(
        "--inject", nargs=2, action="append", metavar=("NAME", "PORT"),
        dest="injects", default=[],
        help="NAME PORT — TCP inject/stream port for a source (optional, repeat)",
    )
    parser.add_argument(
        "--forward", nargs=2, action="append", metavar=("NAME", "PORT"),
        dest="forwards", default=[],
        help="NAME PORT — read-only TCP forward port for raw RX lines (optional, repeat)",
    )
    parser.add_argument(
        "--tab", nargs="+", action="append", metavar="ARG",
        dest="tabs", default=[],
        help=(
            "LABEL SOURCE [SOURCE] — group 1 or 2 sources into a UI tab "
            "(repeat for multiple tabs; omit to get one tab per source)"
        ),
    )
    parser.add_argument("--baudrate", metavar="BAUD", type=int, default=None,
                        help="default baud rate for uart sources without an explicit @baud suffix")
    parser.add_argument("--log-dir", metavar="DIR", default=None, dest="log_dir",
                        help="directory for log files (<log-dir>/<NAME>.log)")
    parser.add_argument("--host", metavar="HOST", default=None,
                        help="bind host for inject ports and WebSocket UI")
    parser.add_argument("--ws-port", metavar="PORT", type=int, default=None, dest="ws_port",
                        help="HTTP/WebSocket port for the browser UI (0 = disabled)")
    parser.add_argument("--ws-ui", metavar="FILE", default=None, dest="ws_ui",
                        help="path to the UI HTML file served at GET /")
    parser.add_argument("-v", "--verbose", action="store_const", const=True, default=None,
                        help="verbose mode: print live lines to stdout, show INFO diagnostics, and prefix lines with [name][source]")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] == "run":
        argv = argv[1:]

    parser = _build_parser()
    args = parser.parse_args(argv)

    cfg = {}
    if args.config:
        try:
            cfg = load_config(args.config)
        except ConfigError as exc:
            parser.error(f"config error: {exc}")

    source_specs = args.sources if args.sources else cfg.get("sources", [])
    inject_specs = args.injects if args.injects else cfg.get("injects", [])
    forward_specs = args.forwards if args.forwards else cfg.get("forwards", [])
    tab_specs = args.tabs if args.tabs else cfg.get("tabs", [])

    baudrate = args.baudrate if args.baudrate is not None else cfg.get("baudrate", 115200)
    logs_root = Path(args.log_dir if args.log_dir is not None else cfg.get("log_dir", "logs/"))
    host = args.host if args.host is not None else cfg.get("host", "127.0.0.1")
    ws_port = args.ws_port if args.ws_port is not None else cfg.get("ws_port", 0)
    ws_ui = args.ws_ui if args.ws_ui is not None else cfg.get("ws_ui", "frontend/index.html")
    verbose = args.verbose if args.verbose is not None else cfg.get("verbose", False)

    logging.basicConfig(
        level=logging.INFO if verbose else logging.WARNING,
        format="%(asctime)s  %(levelname)-8s  %(message)s",
        datefmt="%H:%M:%S",
    )

    if not source_specs:
        parser.error("no sources configured. Use --source ... or --config FILE with sources: ...")

    # --- sources ---
    source_names: list[str] = []
    source_objects: dict[str, LogSource] = {}
    for name, spec in source_specs:
        if name in source_objects:
            parser.error(f"duplicate --source name: {name!r}")
        try:
            source_objects[name] = _parse_source(name, spec, baudrate)
        except argparse.ArgumentTypeError as e:
            parser.error(str(e))
        source_names.append(name)

    # --- injects ---
    inject_ports: dict[str, int] = {}
    for name, port_value in inject_specs:
        if name not in source_objects:
            parser.error(f"--inject {name!r}: no --source with that name")
        try:
            inject_ports[name] = int(port_value)
        except ValueError:
            parser.error(f"--inject {name!r}: port must be an integer, got {port_value!r}")

    # --- forwards ---
    forward_ports: dict[str, list[int]] = {}
    for name, port_value in forward_specs:
        if name not in source_objects:
            parser.error(f"--forward {name!r}: no --source with that name")
        try:
            port = int(port_value)
        except ValueError:
            parser.error(f"--forward {name!r}: port must be an integer, got {port_value!r}")
        forward_ports.setdefault(name, []).append(port)

    # --- tabs ---
    tabs: list[dict] = []
    for tab_entry in tab_specs:
        if len(tab_entry) < 2:
            parser.error(f"--tab requires at least LABEL SOURCE, got: {tab_entry}")
        if len(tab_entry) > 3:
            parser.error(f"--tab takes at most 2 sources per tab, got: {tab_entry}")
        label = tab_entry[0]
        panes = tab_entry[1:]
        for p in panes:
            if p not in source_objects:
                parser.error(f"--tab {label!r}: unknown source {p!r}")
        tabs.append({"label": label, "panes": panes})

    tab_label_by_source: dict[str, str] = {}
    for tab in tabs:
        for pane in tab["panes"]:
            tab_label_by_source[pane] = tab["label"]

    base_session_id = datetime.now().astimezone().strftime("%Y-%m-%d_%H-%M-%S_%f")[:-3]
    session_id = base_session_id
    session_dir = logs_root / session_id
    i = 1
    while session_dir.exists():
        session_id = f"{base_session_id}_{i}"
        session_dir = logs_root / session_id
        i += 1
    session_dir.mkdir(parents=True, exist_ok=True)

    sources = []
    for name in source_names:
        tab_label = tab_label_by_source.get(name, "session")
        log_name = f"{_slug(tab_label)}__{_slug(name)}__{session_id}.log"
        sources.append({
            "name": name,
            "source": source_objects[name],
            "inject_port": inject_ports.get(name),
            "forward_ports": forward_ports.get(name, []),
            "log_file": str(session_dir / log_name),
        })

    LogServer(
        sources,
        tabs,
        session_id=session_id,
        session_dir=str(session_dir),
        logs_root=str(logs_root),
        host=host,
        verbose=verbose,
        ws_port=ws_port,
        ws_ui=ws_ui,
        config_path=args.config,
    ).run_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
