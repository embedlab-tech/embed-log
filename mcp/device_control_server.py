#!/usr/bin/env python3
"""
POC MCP server: device control tools for embed-log.
"""

from __future__ import annotations

import argparse
import json
import socket
import time
from pathlib import Path

from _mcp_stdio import StdioMcpServer, ToolSpec


def _send_json_line(host: str, port: int, payload: dict) -> None:
    data = (json.dumps(payload) + "\n").encode("utf-8")
    with socket.create_connection((host, port), timeout=5) as sock:
        sock.sendall(data)


def _tool_inject_marker(args: dict) -> dict:
    host = args.get("host", "127.0.0.1")
    port = int(args["port"])
    source = args.get("source", "mcp")
    message = args["message"]
    color = args.get("color")
    payload = {"type": "log", "source": source, "message": message}
    if color:
        payload["color"] = color
    _send_json_line(host, port, payload)
    return {"ok": True, "sent": payload, "target": f"{host}:{port}"}


def _tool_send_tx(args: dict) -> dict:
    host = args.get("host", "127.0.0.1")
    port = int(args["port"])
    source = args.get("source", "mcp")
    data = args["data"]
    eol = args.get("eol", "")
    payload = {"type": "tx", "source": source, "data": f"{data}{eol}"}
    _send_json_line(host, port, payload)
    return {"ok": True, "sent": payload, "target": f"{host}:{port}"}


def _tool_tail_log(args: dict) -> dict:
    path = Path(args["path"])
    limit = int(args.get("limit", 50))
    if limit <= 0:
        raise ValueError("limit must be > 0")
    if not path.is_file():
        raise FileNotFoundError(f"log file not found: {path}")
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        lines = fh.readlines()
    tail = [ln.rstrip("\n\r") for ln in lines[-limit:]]
    return {"path": str(path), "count": len(tail), "lines": tail}


def _tool_wait_for_pattern(args: dict) -> dict:
    path = Path(args["path"])
    pattern = args["pattern"]
    timeout_s = float(args.get("timeout_s", 10))
    poll_s = float(args.get("poll_s", 0.2))
    if not path.is_file():
        raise FileNotFoundError(f"log file not found: {path}")
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        text = path.read_text(encoding="utf-8", errors="replace")
        if pattern in text:
            return {"matched": True, "pattern": pattern, "path": str(path)}
        time.sleep(poll_s)
    return {"matched": False, "pattern": pattern, "path": str(path), "timeout_s": timeout_s}


def build_tools() -> dict[str, ToolSpec]:
    return {
        "inject_marker": ToolSpec(
            description="Inject a marker message to an embed-log inject port.",
            input_schema={
                "type": "object",
                "required": ["port", "message"],
                "properties": {
                    "host": {"type": "string", "default": "127.0.0.1"},
                    "port": {"type": "integer"},
                    "source": {"type": "string", "default": "mcp"},
                    "message": {"type": "string"},
                    "color": {"type": "string"},
                },
            },
            handler=_tool_inject_marker,
        ),
        "send_tx": ToolSpec(
            description="Send TX data to a UART-backed source via inject port.",
            input_schema={
                "type": "object",
                "required": ["port", "data"],
                "properties": {
                    "host": {"type": "string", "default": "127.0.0.1"},
                    "port": {"type": "integer"},
                    "source": {"type": "string", "default": "mcp"},
                    "data": {"type": "string"},
                    "eol": {"type": "string", "default": ""},
                },
            },
            handler=_tool_send_tx,
        ),
        "tail_log": ToolSpec(
            description="Read last N lines from a log file.",
            input_schema={
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"},
                    "limit": {"type": "integer", "default": 50},
                },
            },
            handler=_tool_tail_log,
        ),
        "wait_for_pattern": ToolSpec(
            description="Wait until a string appears in a log file.",
            input_schema={
                "type": "object",
                "required": ["path", "pattern"],
                "properties": {
                    "path": {"type": "string"},
                    "pattern": {"type": "string"},
                    "timeout_s": {"type": "number", "default": 10},
                    "poll_s": {"type": "number", "default": 0.2},
                },
            },
            handler=_tool_wait_for_pattern,
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="POC MCP server for embed-log device control")
    parser.parse_args()
    server = StdioMcpServer(
        name="embed-log-device-control",
        version="0.1.0",
        tools=build_tools(),
    )
    server.serve()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
