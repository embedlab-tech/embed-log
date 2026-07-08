"""Tests for the SDK data models."""

from embed_log_sdk.models import LogEntry, Marker


def test_log_entry_from_dict():
    data = {
        "source_id": "DUT_UART",
        "origin": "SERIAL",
        "message": "boot complete",
        "timestamp_iso": "2026-06-14T12:00:00.123Z",
        "line_idx": 42,
        "color": None,
        "is_tx": False,
    }
    entry = LogEntry.from_dict(data)
    assert entry.source_id == "DUT_UART"
    assert entry.origin == "SERIAL"
    assert entry.message == "boot complete"
    assert entry.timestamp_iso == "2026-06-14T12:00:00.123Z"
    assert entry.line_idx == 42
    assert entry.color is None
    assert entry.is_tx is False


def test_log_entry_with_color():
    data = {
        "source_id": "DUT_UART",
        "origin": "pytest",
        "message": "test passed",
        "timestamp_iso": "2026-06-14T12:00:01.000Z",
        "line_idx": 10,
        "color": "green",
        "is_tx": False,
    }
    entry = LogEntry.from_dict(data)
    assert entry.color == "green"


def test_log_entry_tx():
    data = {
        "source_id": "DUT_UART",
        "origin": "pytest",
        "message": "version\r\n",
        "timestamp_iso": "2026-06-14T12:00:02.000Z",
        "line_idx": 11,
        "color": "yellow",
        "is_tx": True,
    }
    entry = LogEntry.from_dict(data)
    assert entry.is_tx is True
    assert entry.origin == "pytest"
    assert entry.color == "yellow"


def test_marker_from_dict():
    data = {
        "paneId": "DUT_UART",
        "lineIdx": 42,
        "endIdx": 42,
        "numTs": 123.45,
        "description": "fatal error",
        "createdAt": "2026-06-14T12:00:00Z",
        "origin": "watcher",
    }
    m = Marker.from_dict(data)
    assert m.pane_id == "DUT_UART"
    assert m.line_idx == 42
    assert m.description == "fatal error"
    assert abs(m.num_ts - 123.45) < 0.001
