"""Tests for backend event subscription support in the Python SDK."""

import json
from unittest.mock import MagicMock

import pytest

from embed_log_sdk.client import EmbedLogClient
from embed_log_sdk.models import Event
from embed_log_sdk.watcher import Watcher, WatcherConfig, WatchRule


class FakeWebSocket:
    def __init__(self):
        self._recv_queue = []
        self._sent = []
        self._closed = False

    def connect(self, url, timeout=None):
        pass

    def send(self, data):
        self._sent.append(data)

    def recv(self):
        if not self._recv_queue:
            raise Exception("no more frames queued")
        raw = self._recv_queue.pop(0)
        if raw.startswith("AUTO:"):
            return self._auto_response(raw[5:])
        return raw

    def settimeout(self, timeout):
        pass

    def close(self):
        self._closed = True

    def queue(self, data):
        self._recv_queue.append(json.dumps(data) if isinstance(data, dict) else data)

    def sent(self):
        return [json.loads(m) for m in self._sent]

    def _auto_response(self, resp_type):
        req = json.loads(self._sent[-1])
        return json.dumps({"type": resp_type, "id": req.get("id")})


@pytest.fixture
def fake_ws(monkeypatch):
    ws = FakeWebSocket()
    monkeypatch.setattr("embed_log_sdk.client.WebSocket", lambda: ws)
    ws.queue({
        "type": "hello.result",
        "id": "hello-init",
        "sources": {"DUT_UART": {"type": "uart", "label": "DUT", "writable": True}},
        "session": {"id": "s1"},
    })
    return ws


def test_subscribe_events_true_sends_correct_command(fake_ws):
    fake_ws.queue("AUTO:subscribe.result")
    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")

    client.subscribe(events=True)

    sub = [m for m in fake_ws.sent() if m.get("type") == "subscribe"][-1]
    assert sub["events"] is True
    assert "sources" not in sub
    client.close()


def test_subscribe_sources_and_events_sends_both(fake_ws):
    fake_ws.queue("AUTO:subscribe.result")
    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")

    client.subscribe(["DUT_UART"], events=True)

    sub = [m for m in fake_ws.sent() if m.get("type") == "subscribe"][-1]
    assert sub["sources"] == ["DUT_UART"]
    assert sub["events"] is True
    client.close()


def test_unsubscribe_events_sends_events_false_with_empty_sources(fake_ws):
    fake_ws.queue("AUTO:unsubscribe.result")
    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")

    client.unsubscribe_events()

    unsub = [m for m in fake_ws.sent() if m.get("type") == "unsubscribe"][-1]
    assert unsub["sources"] == []
    assert unsub["events"] is False
    client.close()


def test_events_yields_parsed_event_objects(fake_ws):
    fake_ws.queue({
        "type": "event",
        "event_id": "fatal_error",
        "source_id": "DUT_UART",
        "severity": "error",
        "timestamp_num": 1718347845123.0,
        "rel_num": 45123.0,
        "line_idx": 42,
        "message": "ZEPHYR FATAL ERROR",
        "captures": ["FATAL ERROR"],
    })
    fake_ws.queue("")
    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")

    events = list(client.events(timeout=0.1))

    assert len(events) == 1
    assert isinstance(events[0], Event)
    assert events[0].event_id == "fatal_error"
    assert events[0].source_id == "DUT_UART"
    assert events[0].severity == "error"
    assert events[0].line_idx == 42
    assert events[0].captures == ["FATAL ERROR"]
    client.close()


def test_event_and_log_entry_messages_interleave_without_loss(fake_ws):
    fake_ws.queue({
        "type": "log.entry",
        "source_id": "DUT_UART",
        "origin": "SERIAL",
        "message": "ordinary log",
        "timestamp_iso": "2026-01-01T00:00:00Z",
        "line_idx": 1,
    })
    fake_ws.queue({
        "type": "event",
        "event_id": "boot_complete",
        "source_id": "DUT_UART",
        "severity": "info",
        "timestamp_num": 10.0,
        "rel_num": 10.0,
        "line_idx": 1,
        "message": "boot complete",
        "captures": ["boot complete"],
    })
    fake_ws.queue("")
    client = EmbedLogClient("ws://127.0.0.1:8080/api/v1/control")

    # events() should preserve the log.entry for entries().
    events = list(client.events(timeout=0.1))
    entries = list(client.entries(timeout=0.1))

    assert [e.event_id for e in events] == ["boot_complete"]
    assert [e.message for e in entries] == ["ordinary log"]
    client.close()


def test_watcher_still_uses_client_side_log_entry_matching():
    client = MagicMock()
    client.entries.return_value = [
        MagicMock(
            source_id="DUT_UART",
            message="ERROR: client-side rule",
            line_idx=7,
            timestamp_iso="2026-01-01T00:00:00Z",
            origin="SERIAL",
        )
    ]
    config = WatcherConfig(
        server_url="ws://127.0.0.1:8080/api/v1/control",
        rules=[WatchRule(name="err", sources=["DUT_UART"], pattern="ERROR", marker=False)],
    )

    watcher = Watcher(config, client)

    assert watcher.run(timeout=0.1) == 1
    client.subscribe.assert_called_once()
    client.events.assert_not_called()
