from __future__ import annotations

import asyncio
import json
import logging
import threading
import webbrowser
from pathlib import Path
from typing import Callable, Optional

import aiohttp
from aiohttp import web
import serial


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
        on_export_session_html: Optional[Callable[[], bool]] = None,
        open_browser: bool = False,
        app_name: str = "embed-log",
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
        self._on_export_session_html = on_export_session_html
        self._no_clients_handle = None
        self._thread: Optional[threading.Thread] = None
        self._started = threading.Event()
        self._start_error: Optional[Exception] = None
        self._stop_async: Optional[asyncio.Event] = None
        self._open_browser = open_browser
        self._app_name = app_name

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
        app.router.add_post("/api/session/export", self._session_export_handler)
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
        if self._open_browser:
            url = f"http://{self._host}:{self._port}/"
            threading.Thread(target=lambda: webbrowser.open(url, new=2), daemon=True).start()
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
        logging.info("API /api/session/current from %s", request.remote)
        return web.json_response(self._session_info)

    async def _session_export_handler(self, request: web.Request) -> web.Response:
        logging.info("API /api/session/export from %s", request.remote)
        if self._on_export_session_html is None:
            return web.json_response({"ok": False, "error": "export unavailable"}, status=503)
        ok = await asyncio.to_thread(self._on_export_session_html)
        status = 200 if ok else 409
        return web.json_response({"ok": ok, "session": self._session_info}, status=status)

    async def _sessions_list_handler(self, request: web.Request) -> web.Response:
        logging.info("API /api/sessions from %s", request.remote)
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
            html_status = "ready" if html_path.is_file() else "pending"
            html_updated_at = None
            html_error = None
            if manifest_path.is_file():
                try:
                    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                    started_at = manifest.get("started_at")
                    tabs = manifest.get("tabs") or []
                    html_status = manifest.get("html_status") or html_status
                    html_updated_at = manifest.get("html_updated_at")
                    html_error = manifest.get("html_error")
                except Exception:
                    pass

            sessions.append({
                "id": session_id,
                "started_at": started_at,
                "html_ready": html_path.is_file(),
                "html_status": html_status,
                "html_updated_at": html_updated_at,
                "html_error": html_error,
                "html": f"/sessions/{session_id}/session.html",
                "manifest": f"/sessions/{session_id}/manifest.json",
                "tabs": tabs,
            })

        return web.json_response({"sessions": sessions, "current": current})

    async def _session_file_handler(self, request: web.Request) -> web.Response:
        if self._sessions_root is None:
            raise web.HTTPNotFound()
        logging.info("GET /sessions/%s/%s from %s", request.match_info.get("session_id"), request.match_info.get("filename"), request.remote)
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
            "app_name": self._app_name,
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
        cmd = msg.get("cmd")
        if cmd == "send_raw":
            name = msg.get("id", "")
            data = msg.get("data", "")
            logging.info("WS cmd send_raw source=%s bytes=%d", name, len(data.encode("utf-8", errors="replace")))
            mgr = self._source_map.get(name)
            if mgr:
                try:
                    mgr._write_source(data.encode("utf-8"), source="UI")
                except (serial.SerialException, TypeError) as exc:
                    logging.warning("send_raw failed for '%s': %s", name, exc)
            return

        if cmd == "export_session_html" and self._on_export_session_html is not None:
            logging.info("WS cmd export_session_html")
            await asyncio.to_thread(self._on_export_session_html)
