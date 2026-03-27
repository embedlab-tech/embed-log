"""
log-viewer — log aggregator with WebSocket UI and TCP inject port.

Usage:
    python3 server.py
        --source READER     uart:/dev/ttyFTDI_A
        --source CONTROLLER uart:/dev/ttyFTDI_B
        --inject READER     5001
        --inject CONTROLLER 5002
        --tab "Devices" READER CONTROLLER
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
import threading
from abc import ABC, abstractmethod
from datetime import datetime
from pathlib import Path
from typing import Callable, Optional

import aiohttp
from aiohttp import web
import serial

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


class FileSource(LogSource):
    """Tails a file, similar to `tail -f`. Retries if the file does not exist."""

    def __init__(self, path: str):
        self.path = path

    def start(self, on_line, stop, name):
        threading.Thread(
            target=self._run, args=(on_line, stop, name),
            daemon=True, name=f"{name}-file",
        ).start()

    def _run(self, on_line, stop, name):
        while not stop.is_set():
            try:
                with open(self.path, encoding="utf-8", errors="replace") as f:
                    f.seek(0, 2)  # start at end
                    while not stop.is_set():
                        line = f.readline()
                        if line:
                            on_line(line.rstrip())
                        else:
                            stop.wait(0.1)
            except FileNotFoundError:
                logging.warning("[%s] file not found: %s — retrying in 3 s", name, self.path)
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

    def __init__(self, html_path: str, host: str, port: int, tabs: list):
        self._html_path = Path(html_path)
        self._host = host
        self._port = port
        self._tabs = tabs          # [{"label": str, "panes": [str, ...]}]
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._clients: set = set()
        self._source_map: dict = {}   # name → SourceManager

    def register_source(self, name: str, mgr) -> None:
        self._source_map[name] = mgr

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
        threading.Thread(target=self._run, daemon=True, name="ws-broadcaster").start()

    def _run(self) -> None:
        self._loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self._loop)
        self._loop.run_until_complete(self._serve())

    async def _serve(self) -> None:
        app = web.Application()
        app.router.add_get("/ws", self._ws_handler)
        app.router.add_get("/", self._index_handler)
        app.router.add_get("/{filename}", self._static_handler)
        runner = web.AppRunner(app)
        await runner.setup()
        site = web.TCPSite(runner, self._host, self._port)
        await site.start()
        logging.info("UI ready at http://%s:%d/  (WebSocket: ws://%s:%d/ws)",
                     self._host, self._port, self._host, self._port)
        await asyncio.Event().wait()

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

    async def _ws_handler(self, request: web.Request) -> web.WebSocketResponse:
        ws = web.WebSocketResponse()
        await ws.prepare(request)
        logging.info("WS client connected: %s", request.remote)

        # Send tab layout BEFORE adding to the broadcast set so that the config
        # message is always the first thing the browser receives — no log entries
        # can arrive before it and trigger premature dynamic tab creation.
        await ws.send_str(json.dumps({"type": "config", "tabs": self._tabs}))
        self._clients.add(ws)

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
            logging.info("WS client disconnected: %s", request.remote)
        return ws

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
        verbose: bool = False,
        broadcaster: Optional[WebSocketBroadcaster] = None,
    ):
        self.name = name
        self.source = source
        self.log_file = Path(log_file)
        self.socket_host = socket_host
        self.inject_port = inject_port
        self.verbose = verbose
        self.broadcaster = broadcaster

        self._queue: queue.Queue[Optional[LogEntry]] = queue.Queue()
        self._stop = threading.Event()
        self._stream_clients: list = []
        self._clients_lock = threading.Lock()

    def start(self) -> None:
        self.log_file.parent.mkdir(parents=True, exist_ok=True)
        threading.Thread(target=self._writer_loop, daemon=True,
                         name=f"{self.name}-writer").start()
        self.source.start(self._on_source_line, self._stop, self.name)
        if self.inject_port:
            threading.Thread(target=self._inject_loop, daemon=True,
                             name=f"{self.name}-inject").start()
        logging.info(
            "[%s] started  source=%s  inject=%s  log=%s",
            self.name,
            type(self.source).__name__,
            f":{self.inject_port}" if self.inject_port else "none",
            self.log_file,
        )

    def stop(self) -> None:
        self._stop.set()
        self._queue.put(None)

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
                print(line, flush=True)
                f.write(line + "\n")
                f.flush()
                if self.broadcaster:
                    self.broadcaster.broadcast(self._ws_payload(entry))
                self._stream_to_clients(self._stream_payload(entry))

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
        host: str = "127.0.0.1",
        verbose: bool = False,
        ws_port: int = 0,
        ws_ui: str = "frontend/index.html",
    ):
        broadcaster: Optional[WebSocketBroadcaster] = None
        if ws_port:
            broadcaster = WebSocketBroadcaster(ws_ui, host, ws_port, tabs)

        self._broadcaster = broadcaster
        self._managers = [
            SourceManager(
                name=s["name"],
                source=s["source"],
                log_file=s["log_file"],
                socket_host=host,
                inject_port=s.get("inject_port"),
                verbose=verbose,
                broadcaster=broadcaster,
            )
            for s in sources
        ]

        if broadcaster:
            for mgr in self._managers:
                broadcaster.register_source(mgr.name, mgr)

    def start(self) -> None:
        if self._broadcaster:
            self._broadcaster.start()
        for mgr in self._managers:
            mgr.start()

    def stop(self) -> None:
        for mgr in self._managers:
            mgr.stop()

    def run_forever(self) -> None:
        self.start()
        stop_event = threading.Event()

        def _handler(sig, frame):
            logging.info("shutting down…")
            self.stop()
            stop_event.set()

        signal.signal(signal.SIGINT, _handler)
        signal.signal(signal.SIGTERM, _handler)
        logging.info("log server running — press Ctrl-C to stop")
        stop_event.wait()


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

_SOURCE_RE = re.compile(r"^(uart|file|udp):(.+)$", re.IGNORECASE)
_UART_BAUD_RE = re.compile(r"^(.+)@(\d+)$")


def _parse_source(name: str, spec: str, default_baudrate: int) -> LogSource:
    m = _SOURCE_RE.match(spec)
    if not m:
        raise argparse.ArgumentTypeError(
            f"--source {name!r}: invalid spec {spec!r}. "
            f"Use uart:/dev/path[@baud], file:/path, or udp:PORT"
        )
    kind, arg = m.group(1).lower(), m.group(2)

    if kind == "uart":
        bm = _UART_BAUD_RE.match(arg)
        if bm:
            return UartSource(bm.group(1), int(bm.group(2)))
        return UartSource(arg, default_baudrate)

    if kind == "file":
        return FileSource(arg)

    # udp
    try:
        return UdpSource(int(arg))
    except ValueError:
        raise argparse.ArgumentTypeError(
            f"--source {name!r}: udp port must be an integer, got {arg!r}"
        )


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="log-viewer — log aggregator with WebSocket UI and TCP inject port.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
        epilog=(
            "Example:\n"
            "  python server.py \\\n"
            "    --source READER     uart:/dev/ttyFTDI_A \\\n"
            "    --source CONTROLLER uart:/dev/ttyFTDI_B \\\n"
            "    --inject READER     5001 \\\n"
            "    --inject CONTROLLER 5002 \\\n"
            "    --tab 'Devices' READER CONTROLLER \\\n"
            "    --ws-port 8080"
        ),
    )
    parser.add_argument(
        "--source", nargs=2, action="append", metavar=("NAME", "TYPE"),
        dest="sources", required=True,
        help=(
            "NAME  uart:/dev/path[@baud] | file:/path | udp:PORT  "
            "— repeat for multiple sources"
        ),
    )
    parser.add_argument(
        "--inject", nargs=2, action="append", metavar=("NAME", "PORT"),
        dest="injects", default=[],
        help="NAME PORT — TCP inject/stream port for a source (optional, repeat)",
    )
    parser.add_argument(
        "--tab", nargs="+", action="append", metavar="ARG",
        dest="tabs", default=[],
        help=(
            "LABEL SOURCE [SOURCE] — group 1 or 2 sources into a UI tab "
            "(repeat for multiple tabs; omit to get one tab per source)"
        ),
    )
    parser.add_argument("--baudrate", metavar="BAUD", type=int, default=115200,
                        help="default baud rate for uart sources without an explicit @baud suffix")
    parser.add_argument("--log-dir", metavar="DIR", default="logs/", dest="log_dir",
                        help="directory for log files (<log-dir>/<NAME>.log)")
    parser.add_argument("--host", metavar="HOST", default="127.0.0.1",
                        help="bind host for inject ports and WebSocket UI")
    parser.add_argument("--ws-port", metavar="PORT", type=int, default=0, dest="ws_port",
                        help="HTTP/WebSocket port for the browser UI (0 = disabled)")
    parser.add_argument("--ws-ui", metavar="FILE", default="frontend/index.html", dest="ws_ui",
                        help="path to the UI HTML file served at GET /")
    parser.add_argument("-v", "--verbose", action="store_true",
                        help="prefix every line with [name][source]")
    return parser


if __name__ == "__main__":
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s  %(levelname)-8s  %(message)s",
        datefmt="%H:%M:%S",
    )

    parser = _build_parser()
    args = parser.parse_args()
    log_dir = Path(args.log_dir)

    # --- sources ---
    source_names: list[str] = []
    source_objects: dict[str, LogSource] = {}
    for name, spec in args.sources:
        if name in source_objects:
            parser.error(f"duplicate --source name: {name!r}")
        try:
            source_objects[name] = _parse_source(name, spec, args.baudrate)
        except argparse.ArgumentTypeError as e:
            parser.error(str(e))
        source_names.append(name)

    # --- injects ---
    inject_ports: dict[str, int] = {}
    for name, port_str in args.injects:
        if name not in source_objects:
            parser.error(f"--inject {name!r}: no --source with that name")
        try:
            inject_ports[name] = int(port_str)
        except ValueError:
            parser.error(f"--inject {name!r}: port must be an integer, got {port_str!r}")

    # --- tabs ---
    tabs: list[dict] = []
    for tab_entry in args.tabs:
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

    sources = [
        {
            "name": name,
            "source": source_objects[name],
            "inject_port": inject_ports.get(name),
            "log_file": str(log_dir / f"{name}.log"),
        }
        for name in source_names
    ]

    LogServer(
        sources,
        tabs,
        host=args.host,
        verbose=args.verbose,
        ws_port=args.ws_port,
        ws_ui=args.ws_ui,
    ).run_forever()
