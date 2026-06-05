from __future__ import annotations

import asyncio
from collections import deque
import json
import logging
import threading
import webbrowser
from pathlib import Path
from typing import Callable

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
        session_info: dict | None = None,
        sessions_root: str | None = None,
        on_all_clients_disconnected: Callable[[], bool | None] | None = None,

        on_export_session_html: Callable[[], bool] | None = None,
        on_rotate_session: Callable[[], dict] | None = None,
        on_save_snippet: Callable[[str, list[str], str, str | None], str | None] | None = None,
        on_save_markers: Callable[[list[dict]], None] | None = None,
        open_browser: bool = False,
        app_name: str = "embed-log",
        theme_defaults: dict | None = None,
        source_labels: dict[str, str] | None = None,
        pane_kinds: dict[str, str] | None = None,
        pane_commands: dict[str, list[str]] | None = None,
        frontend_plugins: dict[str, dict] | None = None,
        pane_plugins: dict[str, list[dict]] | None = None,
        plugin_scripts: dict[str, str] | None = None,
    ):
        self._html_path = Path(html_path)
        self._host = host
        self._port = port
        self._tabs = tabs          # [{"label": str, "panes": [str, ...]}]
        self._loop: asyncio.AbstractEventLoop | None = None
        self._clients: set = set()
        self._broadcast_queue = deque()
        self._broadcast_scheduled = False
        self._broadcast_lock = threading.Lock()
        self._replay_buffer = deque(maxlen=5000)
        self._source_map: dict = {}   # name → SourceManager
        self._sessions_root = Path(sessions_root) if sessions_root else None
        self._session_info = session_info or {}
        self._on_all_clients_disconnected = on_all_clients_disconnected
        self._on_export_session_html = on_export_session_html
        self._on_rotate_session = on_rotate_session
        self._on_save_snippet = on_save_snippet
        self._on_save_markers = on_save_markers
        self._no_clients_handle = None
        self._thread: threading.Thread | None = None
        self._started = threading.Event()
        self._start_error: Exception | None = None
        self._stop_async: asyncio.Event | None = None
        self._open_browser = open_browser
        self._app_name = app_name
        self._theme_defaults = theme_defaults or {}
        self._source_labels = source_labels or {}
        self._pane_kinds = pane_kinds or {}
        self._pane_commands = pane_commands or {}
        self._frontend_plugins = frontend_plugins or {}
        self._pane_plugins = pane_plugins or {}
        self._plugin_scripts = plugin_scripts or {}

    def register_source(self, name: str, mgr) -> None:
        self._source_map[name] = mgr

    def update_session_info(self, updates: dict) -> None:
        self._session_info.update(updates)

    def broadcast(self, msg: dict) -> None:
        """Queue a live UI message without flooding the aiohttp event loop.

        Source writer threads can produce many log lines per second. Scheduling one
        coroutine per line can starve new HTTP/WS handshakes under bursty input, so
        this method coalesces cross-thread notifications into a single drain task.
        """
        loop = self._loop
        if loop is None or loop.is_closed():
            return
        data = json.dumps(msg)
        # Always store in replay buffer so late-connecting clients catch up
        self._replay_buffer.append(data)
        if not self._clients:
            return
        with self._broadcast_lock:
            self._broadcast_queue.append(data)
            if self._broadcast_scheduled:
                return
            self._broadcast_scheduled = True
        try:
            loop.call_soon_threadsafe(self._start_broadcast_drain)
        except RuntimeError:
            with self._broadcast_lock:
                self._broadcast_scheduled = False

    def _start_broadcast_drain(self) -> None:
        asyncio.create_task(self._broadcast_drain_async())

    async def _broadcast_drain_async(self) -> None:
        batch_size = 1000
        while True:
            batch = []
            with self._broadcast_lock:
                for _ in range(batch_size):
                    if not self._broadcast_queue:
                        break
                    batch.append(self._broadcast_queue.popleft())
                if not batch:
                    self._broadcast_scheduled = False
                    return

            if not self._clients:
                continue

            dead = set()
            clients = list(self._clients)
            for data in batch:
                for ws in clients:
                    if ws in dead:
                        continue
                    try:
                        await ws.send_str(data)
                    except Exception:
                        dead.add(ws)
                if dead:
                    self._clients -= dead
                    clients = [ws for ws in clients if ws not in dead]
                    if not clients:
                        break
            await asyncio.sleep(0)

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
        app.router.add_post("/api/session/rotate", self._session_rotate_handler)
        app.router.add_get("/api/sessions", self._sessions_list_handler)
        app.router.add_post("/api/session/snippet", self._snippet_handler)
        app.router.add_get("/sessions/{session_id}/{filename}", self._session_file_handler)
        app.router.add_get("/api/stats", self._stats_handler)
        app.router.add_get("/api/health", self._health_handler)
        app.router.add_get("/", self._index_handler)
        app.router.add_get("/{filename}", self._static_handler)
        runner = web.AppRunner(app)
        await runner.setup()
        site = web.TCPSite(runner, self._host, self._port, reuse_address=True, reuse_port=True)
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

    async def _session_rotate_handler(self, request: web.Request) -> web.Response:
        logging.info("API /api/session/rotate from %s", request.remote)
        if self._on_rotate_session is None:
            return web.json_response({"ok": False, "error": "session rotation unavailable"}, status=503)
        try:
            result = await asyncio.to_thread(self._on_rotate_session)
        except Exception as exc:
            logging.exception("session rotation failed")
            return web.json_response({"ok": False, "error": str(exc)}, status=500)
        return web.json_response({"ok": True, **result})

    async def _snippet_handler(self, request: web.Request) -> web.Response:
        logging.info("API /api/session/snippet from %s", request.remote)
        if self._on_save_snippet is None:
            return web.json_response({"ok": False, "error": "snippet saving unavailable"}, status=503)
        try:
            body = await request.json()
        except Exception:
            return web.json_response({"ok": False, "error": "invalid JSON"}, status=400)

        text = body.get("text", "")
        panes = body.get("panes", [])
        scope = body.get("scope", "exact")
        label = body.get("label", "")

        if not text.strip():
            return web.json_response({"ok": False, "error": "empty text"}, status=400)

        result = await asyncio.to_thread(
            self._on_save_snippet, text, panes, scope, label
        )
        if result is None:
            return web.json_response({"ok": False, "error": "snippet limit exceeded or save failed"}, status=409)

        return web.json_response({"ok": True, "path": result})

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
                except (json.JSONDecodeError, OSError) as exc:
                    logging.debug("manifest parse error for %s: %s", session_id, exc)

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

    async def _health_handler(self, request: web.Request) -> web.Response:
        return web.json_response({"status": "ok"})

    async def _stats_handler(self, request: web.Request) -> web.Response:
        per_source = {}
        for name, mgr in self._source_map.items():
            per_source[name] = mgr.get_stats()
        totals = {
            "enqueued": sum(s["queue"]["enqueued"] for s in per_source.values()),
            "dequeued": sum(s["queue"]["dequeued"] for s in per_source.values()),
            "peak_depth": max((s["queue"]["peak_depth"] for s in per_source.values()), default=0),
            "near_full_events": sum(s["queue"]["near_full_events"] for s in per_source.values()),
        }
        return web.json_response({
            "sources": per_source,
            "totals": totals,
            "session": self._session_info.get("id"),
            "ws_clients": len(self._clients),
        })

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
            "pane_labels": self._source_labels,
            "pane_kinds": self._pane_kinds,
            "pane_commands": self._pane_commands,
            "session": self._session_info,
            "app_name": self._app_name,
            "theme_defaults": self._theme_defaults,
            "frontend_plugins": self._frontend_plugins,
            "pane_plugins": self._pane_plugins,
            "plugin_scripts": self._plugin_scripts,
            "markers": self._session_info.get("markers", []),
        }))
        self._clients.add(ws)
        if self._no_clients_handle is not None:
            self._no_clients_handle.cancel()
            self._no_clients_handle = None
        # Replay buffered log entries to the new client so no history is lost
        for entry in list(self._replay_buffer):
            try:
                await ws.send_str(entry)
            except Exception:
                break

        try:
            async for msg in ws:
                if msg.type == aiohttp.WSMsgType.TEXT:
                    try:
                        await self._handle_command(json.loads(msg.data))
                    except Exception as exc:
                        logging.debug("WS command error: %s", exc)
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
            return

        if cmd == "clear_logs":
            scope = str(msg.get("scope") or "pane")
            pane_id = str(msg.get("id") or "")
            if scope == "all":
                logging.info("WS cmd clear_logs scope=all")
                for mgr in self._source_map.values():
                    mgr.add_ui_clear_marker("all")
                return

            if pane_id:
                mgr = self._source_map.get(pane_id)
                if mgr:
                    logging.info("WS cmd clear_logs scope=pane source=%s", pane_id)
                    mgr.add_ui_clear_marker("pane")
                return

        if cmd == "save_markers" and self._on_save_markers is not None:
            markers = msg.get("markers", [])
            logging.info("WS cmd save_markers count=%d", len(markers))
            await asyncio.to_thread(self._on_save_markers, markers)
            self._session_info["markers"] = markers
            # Broadcast updated markers to all connected clients
            update = json.dumps({"type": "markers_update", "markers": markers})
            for ws in list(self._clients):
                try:
                    await ws.send_str(update)
                except Exception as exc:
                    logging.debug("failed to send markers update to client: %s", exc)
            return

        if cmd == "set_filter":
            name = msg.get("id", "")
            bpf_filter = msg.get("filter", "")
            logging.info("WS cmd set_filter source=%s filter=%r", name, bpf_filter)
            mgr = self._source_map.get(name)
            if mgr:
                error = await asyncio.to_thread(mgr.set_filter, bpf_filter)
                if error:
                    logging.warning("set_filter failed for '%s': %s", name, error)
                # Send ack/nack back to the requesting client
                ws = None
                for client in list(self._clients):
                    try:
                        await client.send_str(json.dumps({
                            "type": "filter_result",
                            "id": name,
                            "filter": bpf_filter,
                            "error": error,
                        }))
                    except Exception:
                        pass
            return
