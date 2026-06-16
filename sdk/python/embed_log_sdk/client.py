"""Sync WebSocket client for the embed-log control API.

Usage:

    from embed_log_sdk import EmbedLogClient

    with EmbedLogClient.from_config("embed-log.yml", origin="pytest") as client:
        # Inject a log entry
        client.inject_log("DUT_UART", "test: assertion passed", color="cyan")

        # Write to UART
        client.tx_write("DUT_UART", "version\\r\\n")

        # Subscribe and read entries
        client.subscribe(["DUT_UART"])
        for entry in client.entries(timeout=5.0):
            print(entry.source_id, entry.message)
"""

from __future__ import annotations

import json
import time
from collections import deque
from pathlib import Path
from typing import Deque, Generator, Optional

from .config import SdkConfig
from .exceptions import (
    ConnectionError,
    NotWritableError,
    ProtocolError,
    ServerError,
    UnknownSourceError,
)
from .models import Event, HelloResult, LogEntry, SessionInfo, SourceInfo

try:
    from websocket import WebSocket, WebSocketException
except ImportError:
    WebSocket = None  # type: ignore


class EmbedLogClient:
    """Synchronous client for the embed-log control WebSocket API.

    Every command is sent with a unique request id.  Responses are matched by
    id and type, so interleaved ``log.entry`` messages do not break command
    handling — they are buffered and surfaced through :meth:`entries`.

    Parameters
    ----------
    command_timeout
        Maximum seconds to wait for a command response before raising
        :class:`~embed_log_sdk.exceptions.ConnectionError`.
    """

    def __init__(
        self,
        url: str,
        origin: str = "sdk",
        sources: Optional[dict[str, SourceInfo]] = None,
        commands: Optional[dict[str, list[str]]] = None,
        connect: bool = True,
        command_timeout: float = 30,
    ):
        self.url = url
        self.origin = origin
        self._sources: dict[str, SourceInfo] = sources or {}
        self._commands: dict[str, list[str]] = commands or {}
        self._ws: Optional[WebSocket] = None
        self._hello_received = False
        self._session: Optional[SessionInfo] = None
        self._command_timeout = command_timeout
        # Buffer for log.entry / other unsolicited messages
        self._msg_buffer: Deque[dict] = deque()
        self._subscribed = False

        if connect:
            self.connect()

    @classmethod
    def from_config(
        cls,
        config_path: str | Path,
        origin: str = "sdk",
    ) -> "EmbedLogClient":
        """Create a client from an embed-log YAML config file.

        Parses the config file for connection details and source metadata,
        then connects and performs the hello handshake.
        """
        config = SdkConfig.from_file(config_path)
        sources = {
            name: SourceInfo(
                name=name,
                source_type=cfg.source_type,
                label=cfg.label,
                writable=cfg.writable,
            )
            for name, cfg in config.sources.items()
        }
        return cls(
            url=config.ws_url,
            origin=origin,
            sources=sources,
            commands=config.commands,
        )

    # ── Connection ──

    def connect(self) -> None:
        if WebSocket is None:
            raise ImportError(
                "websocket-client is required; install with: pip install websocket-client"
            )
        self._ws = WebSocket()
        try:
            self._ws.connect(self.url, timeout=10)
        except Exception as e:
            raise ConnectionError(f"failed to connect to {self.url}: {e}") from e

        # Perform hello handshake
        resp = self._send_and_wait("hello-init", "hello", "hello.result")
        self._hello_received = True
        self._session = SessionInfo(
            id=resp.get("session", {}).get("id", "")
        )
        # Merge runtime sources (authoritative)
        runtime_sources: dict = resp.get("sources", {})
        for name, info in runtime_sources.items():
            if isinstance(info, dict):
                self._sources[name] = SourceInfo(
                    name=name,
                    source_type=info.get("type", ""),
                    label=info.get("label", name),
                    writable=info.get("writable", False),
                )

    def close(self) -> None:
        if self._ws:
            self._ws.close()
            self._ws = None

    def __enter__(self) -> "EmbedLogClient":
        if not self._hello_received:
            self.connect()
        return self

    def __exit__(self, *args) -> None:
        self.close()

    # ── Source helpers ──

    def get_source(self, source_id: str) -> SourceInfo:
        src = self._sources.get(source_id)
        if src is None:
            raise UnknownSourceError("check_source", f"unknown source: {source_id}", source_id)
        return src

    def assert_writable(self, source_id: str) -> None:
        src = self.get_source(source_id)
        if not src.writable:
            raise NotWritableError("tx_write", "source is not writable", source_id)

    # ── Commands ──

    def inject_log(
        self,
        source_id: str,
        message: str,
        color: Optional[str] = None,
    ) -> None:
        """Inject a log entry into the selected source's log stream."""
        self.get_source(source_id)  # validate exists
        body = {
            "source_id": source_id,
            "origin": self.origin,
            "message": message,
        }
        if color:
            body["color"] = color
        self._send_and_wait(
            f"inject-{time.time_ns()}",
            "log.inject",
            "log.inject.result",
            extra=body,
        )

    def tx_write(self, source_id: str, data: str) -> int:
        """Write bytes to a writable source (UART). Returns number of bytes written."""
        self.assert_writable(source_id)
        resp = self._send_and_wait(
            f"tx-{time.time_ns()}",
            "tx.write",
            "tx.result",
            extra={
                "source_id": source_id,
                "origin": self.origin,
                "data": data,
            },
        )
        if resp.get("ok") is True:
            return int(resp.get("bytes", len(data)))
        raise ServerError("tx.write", resp.get("error", "write failed"), source_id)

    def subscribe(self, sources: Optional[list[str]] = None, events: bool = False) -> None:
        """Subscribe to log sources and/or backend-detected events.

        Parameters
        ----------
        sources
            Source ids to receive as ``log.entry`` messages. May be omitted for
            an events-only subscription.
        events
            When true, also receive backend ``event`` messages produced from
            ``.events.yml`` rules. Use :meth:`events` to consume them.
        """
        body: dict = {}
        if sources is not None:
            body["sources"] = sources
        if events:
            body["events"] = True
        self._send_and_wait(
            f"sub-{time.time_ns()}",
            "subscribe",
            "subscribe.result",
            extra=body,
        )
        self._subscribed = True

    def unsubscribe(self, sources: list[str]) -> None:
        """Unsubscribe from sources."""
        self._send_and_wait(
            f"unsub-{time.time_ns()}",
            "unsubscribe",
            "unsubscribe.result",
            extra={"sources": sources},
        )

    def unsubscribe_events(self) -> None:
        """Unsubscribe from backend event messages."""
        self._send_and_wait(
            f"unsub-events-{time.time_ns()}",
            "unsubscribe",
            "unsubscribe.result",
            extra={"sources": [], "events": False},
        )

    def create_marker(
        self,
        source_id: str,
        line_idx: int,
        description: str,
        timestamp_num: Optional[float] = None,
    ) -> None:
        """Create a marker on a log line."""
        self.get_source(source_id)  # validate exists
        body: dict = {
            "source_id": source_id,
            "line_idx": line_idx,
            "description": description,
            "origin": self.origin,
        }
        if timestamp_num is not None:
            body["timestamp_num"] = timestamp_num
        self._send_and_wait(
            f"marker-{time.time_ns()}",
            "marker.create",
            "marker.result",
            extra=body,
        )

    # ── Entry streaming ──

    def entries(self, timeout: Optional[float] = None) -> Generator[LogEntry, None, None]:
        """Yield log entries as they arrive from the server.

        The client must have called subscribe() first.
        Blocks for up to `timeout` seconds between entries.
        If timeout is None, blocks indefinitely.

        Buffered messages from command calls (e.g. ``log.entry`` that arrived
        while ``create_marker`` was waiting) are drained at the top of
        **every** loop iteration, so the watcher does not miss matches.
        Interleaved ``event`` messages stay buffered for :meth:`events`.
        """
        if self._ws is None:
            return

        deadline = (time.time() + timeout) if timeout is not None else None

        while True:
            # Scan the current buffer once. Keep event messages so event
            # consumers do not lose interleaved data.
            for _ in range(len(self._msg_buffer)):
                msg = self._msg_buffer.popleft()
                if msg.get("type") == "log.entry":
                    yield LogEntry.from_dict(msg)
                elif msg.get("type") == "event":
                    self._msg_buffer.append(msg)

            if deadline is not None and time.time() >= deadline:
                break

            try:
                self._ws.settimeout(timeout or 30)
                raw = self._ws.recv()
            except WebSocketException:
                break
            except Exception:
                break

            if not raw:
                break

            try:
                msg = json.loads(raw)
            except json.JSONDecodeError:
                continue

            if msg.get("type") == "log.entry":
                yield LogEntry.from_dict(msg)
            elif msg.get("type") == "event":
                self._msg_buffer.append(msg)

    def events(self, timeout: Optional[float] = None) -> Generator[Event, None, None]:
        """Yield backend-detected events from the control WebSocket stream.

        Call ``subscribe(events=True)`` first (optionally with sources too).
        Log entries encountered while waiting for events are preserved in the
        client's buffer for :meth:`entries`.
        """
        if self._ws is None:
            return

        deadline = (time.time() + timeout) if timeout is not None else None

        while True:
            for _ in range(len(self._msg_buffer)):
                msg = self._msg_buffer.popleft()
                if msg.get("type") == "event":
                    yield Event.from_dict(msg)
                elif msg.get("type") == "log.entry":
                    self._msg_buffer.append(msg)

            if deadline is not None and time.time() >= deadline:
                break

            try:
                self._ws.settimeout(timeout or 30)
                raw = self._ws.recv()
            except WebSocketException:
                break
            except Exception:
                break

            if not raw:
                break

            try:
                msg = json.loads(raw)
            except json.JSONDecodeError:
                continue

            if msg.get("type") == "event":
                yield Event.from_dict(msg)
            elif msg.get("type") == "log.entry":
                self._msg_buffer.append(msg)

    # ── Internal: send + match-by-id ──

    def _next_id(self, prefix: str) -> str:
        return f"{prefix}-{time.time_ns()}"

    def _send_and_wait(
        self,
        request_id: str,
        command_type: str,
        expected_result_type: str,
        extra: Optional[dict] = None,
    ) -> dict:
        """Send a command and wait for the matching response.

        Interleaved ``log.entry`` messages are buffered in ``_msg_buffer``
        so they don't break command handling.
        """
        body = {
            "id": request_id,
            "type": command_type,
        }
        if extra:
            body.update(extra)
        self._send(body)

        # Bounded loop: wait up to command_timeout for the matching response
        deadline = time.time() + self._command_timeout
        self._ws.settimeout(min(5.0, self._command_timeout))  # per-read cap

        while True:
            # Check deadline before and after each recv attempt
            def _raise_timeout():
                raise ConnectionError(
                    f"command '{command_type}' ({request_id}) timed out after "
                    f"{self._command_timeout}s"
                )

            remaining = deadline - time.time()
            if remaining <= 0:
                _raise_timeout()

            # Temporarily reduce socket timeout to remaining time
            self._ws.settimeout(min(5.0, remaining))

            try:
                resp = self._recv_raw()
            except ConnectionError:
                # recv failed — could be a real socket error or the fake WS
                # running out of frames.  Check the deadline; if not expired
                # yet, assume the server hasn't responded and loop back.
                if time.time() >= deadline:
                    _raise_timeout()
                # Small sleep so we don't busy-spin
                time.sleep(0.01)
                continue
            resp_type = resp.get("type", "")
            resp_id = resp.get("id")

            # Server error without expected id -> immediate failure
            if resp_type == "error" and resp_id is None:
                raise ServerError(command_type, resp.get("error", "unknown error"))

            # If it's the expected result-type with our id -> success
            if resp_type == expected_result_type and resp_id == request_id:
                if resp.get("ok") is False:
                    raise ServerError(
                        command_type,
                        resp.get("error", "unknown error"),
                        resp.get("source_id", ""),
                    )
                return resp

            # If it's an unexpected result type with our id -> error
            if resp_id == request_id and resp_type != expected_result_type:
                if resp_type == "error":
                    raise ServerError(command_type, resp.get("error", "unknown error"))
                raise ProtocolError(
                    f"expected {expected_result_type}, got {resp_type}: {resp}"
                )

            # log.entry/event or other unsolicited messages -> buffer
            if resp_type in ("log.entry", "event"):
                self._msg_buffer.append(resp)
                continue

            # Unsolicited result messages with different id -> ignore
            if resp_id is not None and resp_id != request_id:
                if resp_type in ("subscribe.result", "unsubscribe.result",
                                 "log.inject.result", "tx.result", "marker.result"):
                    continue  # stale response from previous call, ignore
                self._msg_buffer.append(resp)
                continue

            # Unexpected message with no matching id -> buffer
            if resp_id is None:
                self._msg_buffer.append(resp)
                continue

            raise ProtocolError(f"unexpected response: {resp}")

    # ── Internal raw send/recv ──

    def _send(self, body: dict) -> None:
        if self._ws is None:
            raise ConnectionError("not connected")
        self._ws.send(json.dumps(body))

    def _recv_raw(self) -> dict:
        if self._ws is None:
            raise ConnectionError("not connected")
        try:
            raw = self._ws.recv()
        except Exception as e:
            raise ConnectionError(f"recv failed: {e}") from e
        if not raw:
            raise ConnectionError("connection closed")
        try:
            return json.loads(raw)
        except json.JSONDecodeError as e:
            raise ProtocolError(f"invalid JSON: {raw}") from e
