"""
Minimal stdio MCP server helper (JSON-RPC over Content-Length frames).

This is intentionally dependency-free so POC servers can run with stdlib only.
"""

from __future__ import annotations

import json
import traceback
from dataclasses import dataclass
from typing import Any, Callable
import sys


JsonDict = dict[str, Any]
ToolHandler = Callable[[JsonDict], Any]


@dataclass
class ToolSpec:
    description: str
    input_schema: JsonDict
    handler: ToolHandler


def _read_message() -> JsonDict | None:
    content_length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if line.lower().startswith(b"content-length:"):
            content_length = int(line.split(b":", 1)[1].strip())
    if content_length is None:
        return None
    payload = sys.stdin.buffer.read(content_length)
    if not payload:
        return None
    return json.loads(payload.decode("utf-8"))


def _write_message(message: JsonDict) -> None:
    payload = json.dumps(message, ensure_ascii=False).encode("utf-8")
    header = f"Content-Length: {len(payload)}\r\n\r\n".encode("ascii")
    sys.stdout.buffer.write(header)
    sys.stdout.buffer.write(payload)
    sys.stdout.buffer.flush()


class StdioMcpServer:
    def __init__(self, name: str, version: str, tools: dict[str, ToolSpec]):
        self.name = name
        self.version = version
        self.tools = tools

    def _ok(self, request_id: Any, result: JsonDict) -> None:
        _write_message({"jsonrpc": "2.0", "id": request_id, "result": result})

    def _err(self, request_id: Any, code: int, message: str) -> None:
        _write_message(
            {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}
        )

    def _handle_initialize(self, request_id: Any) -> None:
        self._ok(
            request_id,
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": self.name, "version": self.version},
            },
        )

    def _handle_tools_list(self, request_id: Any) -> None:
        tools = [
            {"name": n, "description": t.description, "inputSchema": t.input_schema}
            for n, t in self.tools.items()
        ]
        self._ok(request_id, {"tools": tools})

    def _as_content(self, value: Any) -> JsonDict:
        if isinstance(value, str):
            text = value
        else:
            text = json.dumps(value, ensure_ascii=False, indent=2)
        return {"content": [{"type": "text", "text": text}]}

    def _handle_tools_call(self, request_id: Any, params: JsonDict) -> None:
        tool_name = params.get("name")
        arguments = params.get("arguments", {})
        if tool_name not in self.tools:
            self._err(request_id, -32602, f"Unknown tool: {tool_name!r}")
            return
        try:
            out = self.tools[tool_name].handler(arguments)
            self._ok(request_id, self._as_content(out))
        except Exception as exc:
            tb = traceback.format_exc(limit=5)
            self._ok(
                request_id,
                {
                    "content": [
                        {"type": "text", "text": f"Tool error: {exc}\n\n{tb}"}
                    ],
                    "isError": True,
                },
            )

    def serve(self) -> None:
        while True:
            msg = _read_message()
            if msg is None:
                return
            method = msg.get("method")
            request_id = msg.get("id")
            params = msg.get("params", {})

            if method == "initialize":
                self._handle_initialize(request_id)
            elif method == "tools/list":
                self._handle_tools_list(request_id)
            elif method == "tools/call":
                self._handle_tools_call(request_id, params)
            elif method in ("notifications/initialized", "$/cancelRequest"):
                continue
            elif method == "ping":
                self._ok(request_id, {})
            elif request_id is not None:
                self._err(request_id, -32601, f"Method not found: {method!r}")
