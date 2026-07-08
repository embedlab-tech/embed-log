"""Tests for EmbedLogClient using a fake WebSocket."""

import json
from collections import deque
from dataclasses import dataclass, field
from typing import Optional
from unittest.mock import MagicMock, patch

import pytest

from embed_log_sdk.client import EmbedLogClient
from embed_log_sdk.exceptions import (
    ConnectionError as SdkConnectionError,
    NotWritableError,
    ProtocolError,
    ServerError,
    UnknownSourceError,
)


@dataclass
class FakeFrame:
    """A fake WebSocket frame — either a message to send or receive."""

    data: str = ""
    close: bool = False


class FakeWebSocket:
    """Simulates a WebSocket for testing.

    Supports ``AUTO:type:key=val:...`` syntax for ``queue_send``:
    automatically echoes back the client's request id.

    Example::

        ws = FakeWebSocket()
        ws.queue_send('{"type":"hello.result","sources":{},"session":{"id":"s1"}}')
        ws.queue_send("AUTO:log.inject.result:ok=True")
    """

    def __init__(self):
        self._recv_queue: list[str] = []
        self._sent: list[str] = []
        self._closed = False

    def connect(self, url, timeout=None):
        pass

    def send(self, data: str):
        self._sent.append(data)

    def recv(self):
        if self._recv_queue:
            raw = self._recv_queue.pop(0)
            # AUTO response: read the last client message to extract its id
            if raw.startswith("AUTO:"):
                return self._build_auto_response(raw[5:], self._sent[-1] if self._sent else "")
            return raw
        raise Exception("no more frames queued")

    def settimeout(self, t):
        pass

    def close(self):
        self._closed = True

    # --- internal ---

    def _build_auto_response(self, spec: str, last_request: str) -> str:
        """Build a JSON response from AUTO spec, extracting id from last_request."""
        parts = spec.split(":")
        resp_type = parts[0]
        result: dict = {"type": resp_type}
        # Extract id from the last client message
        try:
            req = json.loads(last_request)
            if "id" in req:
                result["id"] = req["id"]
        except (json.JSONDecodeError, IndexError):
            pass
        # Parse key=val pairs
        for pair in parts[1:]:
            if "=" in pair:
                k, v = pair.split("=", 1)
                # Parse booleans
                if v == "True":
                    result[k] = True
                elif v == "False":
                    result[k] = False
                else:
                    try:
                        result[k] = int(v)
                    except ValueError:
                        result[k] = v
        return json.dumps(result)

    # --- test helpers ---

    def queue_send(self, data: str):
        """Queue a server response."""
        if isinstance(data, dict):
            data = json.dumps(data)
        self._recv_queue.append(data)

    def sent(self) -> list[dict]:
        """Return all messages the client sent, parsed."""
        return [json.loads(m) for m in self._sent]

    def last_sent(self) -> Optional[dict]:
        if self._sent:
            return json.loads(self._sent[-1])
        return None


@pytest.fixture
def fake_ws():
    """Create a FakeWebSocket and patch websocket.WebSocket."""
    ws = FakeWebSocket()
    # Patch websocket.create_connection or the class itself
    patcher = patch("embed_log_sdk.client.WebSocket", return_value=ws)
    patcher.start()
    yield ws
    patcher.stop()


# ── Hello handshake ──


def test_hello_handshake_merges_runtime_sources(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result",
        "id": "hello-init",
        "sources": {
            "DUT_UART": {"type": "uart", "label": "DUT", "writable": True},
            "PYTEST": {"type": "udp", "label": "Pytest", "writable": False},
        },
        "session": {"id": "s1"},
    }))

    client = EmbedLogClient(
        "ws://127.0.0.1:8080/api/v1/control",
        sources={
            "DUT_UART": MagicMock(name="DUT_UART", writable=True),
        },
    )
    assert "DUT_UART" in client._sources
    assert "PYTEST" in client._sources
    assert client._session is not None
    assert client._session.id == "s1"
    client.close()


def test_hello_handshake_requires_hello_result(fake_ws):
    fake_ws.queue_send(json.dumps({"type": "error", "error": "bad"}))

    with pytest.raises(ServerError, match="bad"):
        EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")


# ── inject_log ──


def test_inject_log_success(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    # Use AUTO_ID so the helper matches any id
    fake_ws.queue_send("AUTO:log.inject.result")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    client.inject_log("DUT_UART", "hello", color="cyan")
    # Verify the sent message
    sent = fake_ws.sent()
    inject_msg = [m for m in sent if m.get("type") == "log.inject"]
    assert len(inject_msg) == 1
    assert inject_msg[0]["source_id"] == "DUT_UART"
    assert inject_msg[0]["color"] == "cyan"
    client.close()


def test_inject_log_unknown_source(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    with pytest.raises(UnknownSourceError):
        client.inject_log("NONEXISTENT", "test")
    client.close()


def test_inject_log_server_error(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    fake_ws.queue_send("AUTO:log.inject.result:ok=False:error=source queue closed")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    with pytest.raises(ServerError, match="source queue closed"):
        client.inject_log("DUT_UART", "test")
    client.close()


# ── tx_write ──


def test_tx_write_success(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    fake_ws.queue_send("AUTO:tx.result:ok=True:source_id=DUT_UART:bytes=9")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    written = client.tx_write("DUT_UART", "version\r\n")
    assert written == 9
    client.close()


def test_tx_write_non_writable(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"PYTEST": {"type": "udp", "label": "Pytest", "writable": False}},
        "session": {"id": "s1"},
    }))

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    with pytest.raises(NotWritableError):
        client.tx_write("PYTEST", "data")
    client.close()


def test_tx_write_failure(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    fake_ws.queue_send("AUTO:tx.result:ok=False:source_id=DUT_UART:error=serial port disconnected")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    with pytest.raises(ServerError, match="serial port disconnected"):
        client.tx_write("DUT_UART", "data")
    client.close()


# ── create_marker ──


def test_create_marker_success(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    fake_ws.queue_send("AUTO:marker.result:ok=True:source_id=DUT_UART")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    client.create_marker("DUT_UART", 42, "test marker")
    client.close()


def test_create_marker_failure(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    fake_ws.queue_send("AUTO:marker.result:ok=False:source_id=DUT_UART:error=line_idx out of range")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    with pytest.raises(ServerError, match="line_idx out of range"):
        client.create_marker("DUT_UART", 999, "bad")
    client.close()


# ── Interleaved log.entry before command result ──


def test_interleaved_log_entry_buffered_during_command(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    # Server sends a log.entry first, then the inject result
    fake_ws.queue_send(json.dumps({
        "type": "log.entry",
        "source_id": "DUT_UART", "origin": "SERIAL", "message": "interleaved",
        "timestamp_iso": "2026-01-01T00:00:00Z", "line_idx": 1,
    }))
    fake_ws.queue_send("AUTO:log.inject.result:ok=True")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    # This should not raise — the log.entry is buffered
    client.inject_log("DUT_UART", "hello")

    # Now entries() should yield the buffered log.entry
    entries = list(client.entries(timeout=0.1))
    assert len(entries) == 1
    assert entries[0].message == "interleaved"
    client.close()


# ── subscribe ──


def test_subscribe_sends_correct_message(fake_ws):
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    fake_ws.queue_send("AUTO:subscribe.result")

    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")
    client.subscribe(["DUT_UART"])
    sent = fake_ws.sent()
    sub_msgs = [m for m in sent if m.get("type") == "subscribe"]
    assert len(sub_msgs) == 1
    assert sub_msgs[0]["sources"] == ["DUT_UART"]
    assert client._subscribed
    client.close()


# ── Timeout ──


def test_command_timed_out_after_deadline(fake_ws):
    """If the server never sends a response, _send_and_wait raises after the deadline."""
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    # No response queued for the inject command — will hang without timeout

    client = EmbedLogClient(
        "ws://127.0.0.1:8080/api/v1/control",
        command_timeout=0.2,  # short timeout for testing
    )
    with pytest.raises(SdkConnectionError, match="timed out"):
        client.inject_log("DUT_UART", "hello")
    client.close()


def test_stale_wrong_id_result_does_not_match(fake_ws):
    """A stale result from a previous command with a different id should not satisfy _send_and_wait."""
    fake_ws.queue_send(json.dumps({
        "type": "hello.result", "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    }))
    # A stale tx.result for a different request id arrives first
    fake_ws.queue_send(json.dumps({
        "type": "tx.result", "id": "tx-stale", "ok": True,
    }))
    # Then the actual response
    fake_ws.queue_send("AUTO:log.inject.result:ok=True")

    client = EmbedLogClient(
        "ws://127.0.0.1:8080/api/v1/control",
        command_timeout=5,
    )
    # Should succeed despite the stale message
    client.inject_log("DUT_UART", "hello")
    client.close()


def test_entries_drains_buffer_every_iteration():
    """Direct test: entries() drains _msg_buffer at the start of every loop iteration."""
    from collections import deque
    from unittest.mock import MagicMock

    ws = MagicMock()
    ws.recv.side_effect = [""]  # empty string to break the loop
    ws.settimeout = MagicMock()

    client = EmbedLogClient.__new__(EmbedLogClient)
    client._ws = ws
    client._msg_buffer = deque()
    client._command_timeout = 30

    # Simulate buffered entries that arrived during a previous command
    client._msg_buffer.append({"type": "log.entry", "source_id": "DUT_UART",
                               "origin": "SERIAL", "message": "buffered-1",
                               "timestamp_iso": "2026-01-01T00:00:00Z", "line_idx": 1})
    client._msg_buffer.append({"type": "log.entry", "source_id": "DUT_UART",
                               "origin": "SERIAL", "message": "buffered-2",
                               "timestamp_iso": "2026-01-01T00:00:01Z", "line_idx": 2})
    # A non-entry message that should not be yielded
    client._msg_buffer.append({"type": "subscribe.result"})

    entries = list(client.entries(timeout=0.1))
    assert len(entries) == 2
    assert entries[0].message == "buffered-1"
    assert entries[1].message == "buffered-2"
    assert client._msg_buffer == deque()  # all drained
