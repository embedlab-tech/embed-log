"""Tests for the watcher module."""

from pathlib import Path

import pytest
import yaml

from embed_log_sdk.watcher import WatcherConfig, WatchRule


def test_watch_rule_compiles_pattern():
    rule = WatchRule(name="test", sources=["DUT_UART"], pattern="ERROR", marker=False)
    assert rule._compiled is not None


def test_watch_rule_invalid_pattern():
    with pytest.raises(ValueError, match="invalid regex"):
        WatchRule(name="bad", sources=["DUT_UART"], pattern="[unclosed", marker=False)


def test_watch_rule_match():
    rule = WatchRule(name="err", sources=["DUT_UART"], pattern="ERROR", marker=False)

    class FakeEntry:
        source_id = "DUT_UART"
        message = "something ERROR happened"
        line_idx = 5
        timestamp_iso = "2026-06-14T12:00:00Z"
        origin = "SERIAL"

    result = rule.match(FakeEntry())  # type: ignore
    assert result is not None
    assert result["watch"] == "err"
    assert result["line_idx"] == 5


def test_watch_rule_no_match():
    rule = WatchRule(name="err", sources=["DUT_UART"], pattern="ERROR", marker=False)

    class FakeEntry:
        source_id = "DUT_UART"
        message = "all good"
        line_idx = 1
        timestamp_iso = "2026-06-14T12:00:00Z"
        origin = "SERIAL"

    assert rule.match(FakeEntry()) is None  # type: ignore


def test_watch_rule_wrong_source():
    rule = WatchRule(name="err", sources=["PYTEST"], pattern="ERROR", marker=False)

    class FakeEntry:
        source_id = "DUT_UART"
        message = "ERROR"
        line_idx = 1
        timestamp_iso = "2026-06-14T12:00:00Z"
        origin = "SERIAL"

    assert rule.match(FakeEntry()) is None  # type: ignore


def test_watch_rule_named_groups():
    rule = WatchRule(name="wd", sources=["DUT_UART"], pattern=r"watchdog: (?P<seconds>\d+)s", marker=False)

    class FakeEntry:
        source_id = "DUT_UART"
        message = "watchdog: 5s"
        line_idx = 1
        timestamp_iso = "2026-06-14T12:00:00Z"
        origin = "SERIAL"

    result = rule.match(FakeEntry())  # type: ignore
    assert result is not None
    assert result["groups"] == {"seconds": "5"}


def test_watcher_config_parsing(tmp_path):
    yml_content = """
server:
  url: ws://127.0.0.1:8080/api/v1/control

output:
  path: matches.jsonl

watch:
  - name: fatal
    sources: [DUT_UART]
    pattern: "ZEPHYR FATAL ERROR"
    marker: true

  - name: watchdog
    sources: [DUT_UART]
    pattern: "watchdog: \\\\d+s"
    marker: false
"""
    config_file = tmp_path / "watcher.yml"
    config_file.write_text(yml_content)

    config = WatcherConfig.from_file(config_file)
    assert config.server_url == "ws://127.0.0.1:8080/api/v1/control"
    assert config.output_path == config_file.resolve().parent / "matches.jsonl"
    assert len(config.rules) == 2
    assert config.rules[0].name == "fatal"
    assert config.rules[0].marker is True
    assert config.rules[1].name == "watchdog"
    assert config.rules[1].marker is False


def test_evidence_written_to_jsonl(tmp_path):
    """Match evidence is written as JSONL to the configured output path."""
    import json
    from unittest.mock import MagicMock

    from embed_log_sdk.watcher import Watcher, WatcherConfig

    config = WatcherConfig(
        server_url="ws://127.0.0.1:8080/api/v1/control",
        output_path=tmp_path / "evidence.jsonl",
        rules=[
            WatchRule(name="err", sources=["DUT_UART"], pattern="ERROR", marker=False),
        ],
    )
    client = MagicMock()
    client.entries.return_value = [
        MagicMock(
            source_id="DUT_UART", message="ERROR: something broke",
            line_idx=5, timestamp_iso="2026-01-01T00:00:00Z", origin="SERIAL",
        ),
    ]

    watcher = Watcher(config, client)
    count = watcher.run(timeout=0.1)
    watcher.close()

    assert count == 1
    lines = (tmp_path / "evidence.jsonl").read_text().strip().split("\n")
    assert len(lines) == 1
    evidence = json.loads(lines[0])
    assert evidence["watch"] == "err"
    assert evidence["source_id"] == "DUT_UART"
    assert evidence["line_idx"] == 5


def test_subscribes_to_union_of_source_rules():
    """Watcher subscribes to the union of all sources across all rules."""
    from unittest.mock import MagicMock

    from embed_log_sdk.watcher import Watcher, WatcherConfig

    config = WatcherConfig(
        server_url="ws://127.0.0.1:8080/api/v1/control",
        rules=[
            WatchRule(name="a", sources=["DUT_UART", "PYTEST"], pattern="ERR", marker=False),
            WatchRule(name="b", sources=["PYTEST"], pattern="WARN", marker=False),
            WatchRule(name="c", sources=["HOST"], pattern="INFO", marker=False),
        ],
    )
    client = MagicMock()
    client.entries.return_value = []

    watcher = Watcher(config, client)
    watcher.run(timeout=0.1)

    # Should subscribe to the union
    expected_sources = {"DUT_UART", "PYTEST", "HOST"}
    # Check that subscribe was called with the expected sources
    call_args = client.subscribe.call_args
    assert call_args is not None, "subscribe was never called"
    subscribed_sources = set(call_args[0][0])
    assert subscribed_sources == expected_sources


def test_marker_created_only_for_marker_rules():
    """marker.create is called only for rules with marker=True."""
    from unittest.mock import MagicMock

    from embed_log_sdk.watcher import Watcher, WatcherConfig

    config = WatcherConfig(
        server_url="ws://127.0.0.1:8080/api/v1/control",
        rules=[
            WatchRule(name="mark-on", sources=["DUT_UART"], pattern="MATCH", marker=True),
            WatchRule(name="mark-off", sources=["DUT_UART"], pattern="MATCH", marker=False),
        ],
    )
    client = MagicMock()
    client.entries.return_value = [
        MagicMock(
            source_id="DUT_UART", message="MATCH this",
            line_idx=10, timestamp_iso="2026-01-01T00:00:00Z", origin="SERIAL",
        ),
    ]

    watcher = Watcher(config, client)
    count = watcher.run(timeout=0.1)

    assert count == 2  # both match
    # marker.create should be called exactly once (for mark-on only)
    assert client.create_marker.call_count == 1
    call_args = client.create_marker.call_args[1]
    assert call_args["source_id"] == "DUT_UART"
    assert call_args["line_idx"] == 10
    assert call_args["description"] == "mark-on"


def test_timeout_behavior_is_deterministic():
    """Watcher.run respects timeout and returns match count."""
    from unittest.mock import MagicMock

    from embed_log_sdk.watcher import Watcher, WatcherConfig

    config = WatcherConfig(
        server_url="ws://127.0.0.1:8080/api/v1/control",
        rules=[],
    )
    client = MagicMock()
    client.entries.return_value = []

    watcher = Watcher(config, client)
    count = watcher.run(timeout=0.01)  # very short timeout
    assert count == 0  # no matches, but should return cleanly
