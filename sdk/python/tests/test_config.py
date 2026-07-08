"""Tests for the SDK YAML config parser."""

import json
import os
import tempfile
from pathlib import Path

import pytest
import yaml

from embed_log_sdk.config import SdkConfig
from embed_log_sdk.exceptions import ConfigError


def test_config_parses_server_url():
    yml = """
server:
  host: 0.0.0.0
  ws_port: 9090
sources: []
"""
    cfg = SdkConfig.from_dict(yaml.safe_load(yml))
    assert cfg.server.host == "0.0.0.0"
    assert cfg.server.ws_port == 9090
    assert cfg.ws_url == "ws://0.0.0.0:9090/api/v1/control"


def test_config_defaults():
    yml = "sources: []"
    cfg = SdkConfig.from_dict(yaml.safe_load(yml))
    assert cfg.server.host == "127.0.0.1"
    assert cfg.server.ws_port == 8080


def test_config_parses_sources():
    yml = """
sources:
  - name: DUT_UART
    type: uart
    label: DUT
  - name: PYTEST
    type: udp
    label: Pytest
"""
    cfg = SdkConfig.from_dict(yaml.safe_load(yml))
    assert "DUT_UART" in cfg.sources
    assert "PYTEST" in cfg.sources
    assert cfg.sources["DUT_UART"].source_type == "uart"
    assert cfg.sources["PYTEST"].source_type == "udp"


def test_uart_is_writable():
    yml = """
sources:
  - name: DUT_UART
    type: uart
  - name: PYTEST
    type: udp
  - name: LOG_FILE
    type: file
"""
    cfg = SdkConfig.from_dict(yaml.safe_load(yml))
    assert cfg.is_writable("DUT_UART") is True
    assert cfg.is_writable("PYTEST") is False
    assert cfg.is_writable("LOG_FILE") is False


def test_config_file_not_found():
    with pytest.raises(ConfigError, match="not found"):
        SdkConfig.from_file("/nonexistent/path/to/config.yml")


def test_companion_commands_loaded(tmp_path):
    config_file = tmp_path / "test_config.yml"
    config_file.write_text("sources:\n  - name: DUT_UART\n    type: uart\n")

    cmd_file = tmp_path / "test_config.commands.yml"
    cmd_file.write_text("sources:\n  DUT_UART:\n    - \"help\\r\\n\"\n    - \"version\\r\\n\"\n")

    cfg = SdkConfig.from_file(config_file)
    assert "DUT_UART" in cfg.commands
    assert len(cfg.commands["DUT_UART"]) == 2


def test_fallback_commands_loaded_when_specific_absent(tmp_path):
    config_file = tmp_path / "test_config.yml"
    config_file.write_text("sources:\n  - name: DUT_UART\n    type: uart\n")

    fallback = tmp_path / "embed-log.commands.yml"
    fallback.write_text("sources:\n  DUT_UART:\n    - 'fallback_cmd\\n'\n")

    cfg = SdkConfig.from_file(config_file)
    assert cfg.commands.get("DUT_UART") == ["fallback_cmd\\n"]


def test_unknown_source_commands_ignored(tmp_path):
    config_file = tmp_path / "test_config.yml"
    config_file.write_text("sources:\n  - name: DUT_UART\n    type: uart\n")

    cmd_file = tmp_path / "test_config.commands.yml"
    cmd_file.write_text("sources:\n  DUT_UART:\n    - 'ok\\n'\n  NONEXISTENT:\n    - 'bad\\n'\n")

    cfg = SdkConfig.from_file(config_file)
    assert "NONEXISTENT" not in cfg.commands
    assert "DUT_UART" in cfg.commands


def test_config_source_names():
    yml = """
sources:
  - name: A
    type: uart
  - name: B
    type: udp
"""
    cfg = SdkConfig.from_dict(yaml.safe_load(yml))
    assert set(cfg.source_names()) == {"A", "B"}
