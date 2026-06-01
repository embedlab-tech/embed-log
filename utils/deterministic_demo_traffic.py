#!/usr/bin/env python3
"""
Demo traffic generator for embed-log.

Generates simulated UDP log traffic for demo and test scenarios.
Two content modes:
  test    — TEST-prefixed deterministic patterns for Playwright UI tests
  curated — realistic REST API testing story for the website demo

--content test example:
    python utils/deterministic_demo_traffic.py \
        --content test \
        --udp SENSOR_A=127.0.0.1:6000 \
        --udp SENSOR_B=127.0.0.1:6001 \
        --udp SENSOR_C=127.0.0.1:6002 \
        --inject SENSOR_A=127.0.0.1:5001 \
        --inject SENSOR_B=127.0.0.1:5002 \
        --inject SENSOR_C=127.0.0.1:5003 \
        --tick-ms 100 \
        --cycles 0

--content curated example (default):
    python utils/deterministic_demo_traffic.py \
        --udp SENSOR_A=127.0.0.1:6000 \
        --udp SENSOR_B=127.0.0.1:6001 \
        --udp SENSOR_C=127.0.0.1:6002 \
        --udp SENSOR_D=127.0.0.1:6004 \
        --tick-ms 300 \
        --cycles 16

When --cbor is given, datagrams are encoded as CBOR maps instead of text
lines.  Each CBOR map is sent as one datagram and can be consumed by a
UDP source with parser type ``cbor-datagram``.
"""

from __future__ import annotations

import argparse
import json
import socket
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

import cbor2

ROOT_DIR = Path(__file__).resolve().parents[1]
if str(ROOT_DIR) not in sys.path:
    sys.path.insert(0, str(ROOT_DIR))

from backend.log_client import LogClient


@dataclass(frozen=True)
class Target:
    name: str
    host: str
    port: int


def _parse_named_target(value: str) -> Target:
    if "=" not in value:
        raise argparse.ArgumentTypeError(f"expected NAME=HOST:PORT, got {value!r}")
    name, addr = value.split("=", 1)
    name = name.strip()
    if not name:
        raise argparse.ArgumentTypeError(f"target name is empty in {value!r}")
    if ":" not in addr:
        raise argparse.ArgumentTypeError(f"expected NAME=HOST:PORT, got {value!r}")
    host, port_s = addr.rsplit(":", 1)
    if not host:
        raise argparse.ArgumentTypeError(f"target host is empty in {value!r}")
    try:
        port = int(port_s)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(f"target port must be integer in {value!r}") from exc
    if not (1 <= port <= 65535):
        raise argparse.ArgumentTypeError(f"target port out of range in {value!r}")
    return Target(name=name, host=host, port=port)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate deterministic embed-log demo traffic for UI tests."
    )
    parser.add_argument(
        "--udp",
        action="append",
        type=_parse_named_target,
        default=[],
        metavar="NAME=HOST:PORT",
        help="UDP target for a source. Repeat for multiple sources.",
    )
    parser.add_argument(
        "--inject",
        action="append",
        type=_parse_named_target,
        default=[],
        metavar="NAME=HOST:PORT",
        help="Optional inject target for a source. Repeat for multiple sources.",
    )
    parser.add_argument(
        "--tick-ms",
        type=float,
        default=100.0,
        help="Milliseconds between deterministic ticks (default: 100).",
    )
    parser.add_argument(
        "--cycles",
        type=int,
        default=0,
        help="Number of ticks to send, 0 means run forever (default: 0).",
    )
    parser.add_argument(
        "--connect-timeout",
        type=float,
        default=30.0,
        help="Inject client connection timeout in seconds (default: 30).",
    )
    parser.add_argument(
        "--cbor",
        action="store_true",
        help="Encode datagrams as CBOR maps instead of text lines.",
    )
    parser.add_argument(
        "--content",
        choices=["test", "curated"],
        default="curated",
        help="Content mode: 'test' for UI test patterns, 'curated' for REST API demo story (default: curated).",
    )
    return parser.parse_args()


def _msg(src: str, tick: int, seq: int, kind: str, message: str) -> str:
    return f'TEST src={src} tick={tick:03d} seq={seq:04d} kind={kind} msg="{message}"'


def _embedded_timestamp(tick: int) -> str:
    # Fixed timestamp is intentional: tests can verify cleanup of duplicated
    # payload timestamps without relying on wall-clock time.
    base = datetime(2026, 1, 1, 0, 0, 0, tzinfo=timezone.utc)
    ts = base.replace(second=tick % 60).isoformat(timespec="milliseconds").replace("+00:00", "Z")
    return ts
# ── CoAP message hex generator ─────────────────────────────────────────
# Encodes valid CoAP packets as space-separated hex strings so the
# frontend hex-coap plugin can detect and decode them.

def _coap_option(delta: int, value_bytes: bytes) -> bytes:
    """Encode a single CoAP option (header + value)."""
    length = len(value_bytes)
    if delta < 13:
        delta_init = delta
        delta_ext = b''
    elif delta < 269:
        delta_init = 13
        delta_ext = bytes([delta - 13])
    else:
        delta_init = 14
        delta_ext = (delta - 269).to_bytes(2, 'big')

    if length < 13:
        len_init = length
        len_ext = b''
    elif length < 269:
        len_init = 13
        len_ext = bytes([length - 13])
    else:
        len_init = 14
        len_ext = (length - 269).to_bytes(2, 'big')

    first_byte = (delta_init << 4) | len_init
    return first_byte.to_bytes(1, 'big') + delta_ext + len_ext + value_bytes


# CoAP message types (RFC 7252 §3)
_TYPE_CON = 0
_TYPE_NON = 1
_TYPE_ACK = 2
_TYPE_RST = 3


def _gen_coap_hex(
    type_val: int,
    code: int,
    msg_id: int,
    *,
    token: bytes = b'',
    uri_paths: list[str] | None = None,
    uri_queries: list[str] | None = None,
    payload: bytes = b'',
    extra_options: list[tuple[int, bytes]] | None = None,
) -> str:
    """Build a CoAP message and return a space-separated uppercased hex string."""
    parts = bytearray()
    first_byte = (1 << 6) | (type_val << 4) | (len(token) & 0x0F)
    parts.append(first_byte)
    parts.append(code)
    parts.append((msg_id >> 8) & 0xFF)
    parts.append(msg_id & 0xFF)
    parts.extend(token)

    # Collect all options, sort by number (CoAP requires ascending order)
    opts: list[tuple[int, bytes]] = []
    for path in (uri_paths or []):
        opts.append((11, path.encode('utf-8')))
    for query in (uri_queries or []):
        opts.append((15, query.encode('utf-8')))
    for opt_num, opt_val in (extra_options or []):
        opts.append((opt_num, opt_val))
    opts.sort(key=lambda x: x[0])

    prev = 0
    for opt_num, opt_val in opts:
        parts.extend(_coap_option(opt_num - prev, opt_val))
        prev = opt_num

    if payload:
        parts.append(0xFF)
        parts.extend(payload)

    return ' '.join(f'{b:02X}' for b in parts)


# Pre-generated test messages (15 variants; loops every 15 ticks)
COAP_DEMO_HEX_LIST: list[str] = [
    _gen_coap_hex(_TYPE_CON, 1, 0x1234, uri_paths=['foo', 'bar']),
    _gen_coap_hex(_TYPE_CON, 1, 0x5678, uri_paths=['temp']),
    _gen_coap_hex(_TYPE_NON, 2, 0x9ABC, uri_paths=['sensors', 'temperature'], payload=b'23.5'),
    _gen_coap_hex(_TYPE_ACK, 69, 0x1111, payload=b'OK'),
    _gen_coap_hex(_TYPE_CON, 1, 0x2222, uri_queries=['type=sensor']),
    _gen_coap_hex(_TYPE_RST, 0, 0x3333),
    _gen_coap_hex(_TYPE_CON, 2, 0x4444, token=b'\xAB\xCD', uri_paths=['cfg'], payload=b'{"rate":100}'),
    _gen_coap_hex(_TYPE_NON, 1, 0x5555, uri_paths=['.well-known', 'core']),
    _gen_coap_hex(_TYPE_CON, 4, 0x6666, uri_paths=['sessions', '42']),
    _gen_coap_hex(_TYPE_ACK, 132, 0x7777),
    # New: options from extensions
    _gen_coap_hex(_TYPE_CON, 1, 0x4488, uri_paths=['temp'], extra_options=[(6, b'\x2A')]),
    _gen_coap_hex(_TYPE_CON, 1, 0x5599, uri_paths=['data'], extra_options=[(23, b'\x02')]),
    _gen_coap_hex(_TYPE_CON, 1, 0x66AA, uri_paths=['sensor'], extra_options=[(9, b'\xDE\xAD\xBE\xEF')], payload=b'\x01\x02\x03'),
    _gen_coap_hex(_TYPE_CON, 1, 0x77BB, extra_options=[(4, b'\xA1\xB2\xC3\xD4')], uri_paths=['cfg']),
    _gen_coap_hex(_TYPE_ACK, 69, 0x88CC, extra_options=[(14, b'\x00\x78')], payload=b'ok'),
]

def build_udp_lines(src: str, tick: int, seq_start: int) -> tuple[list[str], int]:
    """Return deterministic lines for one source/tick and next sequence value."""
    seq = seq_start
    lines: list[str] = []

    lines.append(_msg(src, tick, seq, "sync", f"{src} synchronized step {tick:03d}"))
    seq += 1

    if src == "SENSOR_A" and tick % 8 == 4:
        hex_msg = COAP_DEMO_HEX_LIST[tick % len(COAP_DEMO_HEX_LIST)]
        lines.append(_msg(src, tick, seq, "coap-demo", f"coap rx: frame AA 55 payload {hex_msg}"))
        seq += 1

    # ── SENSOR_COAP sends bare hex lines (no TEST envelope) for the
    #     dedicated CoAP tab where the plugin acts on the hex directly.
    if src == "SENSOR_COAP":
        hex_msg = COAP_DEMO_HEX_LIST[(tick - 1) % len(COAP_DEMO_HEX_LIST)]
        lines.append(f"coap {hex_msg}")
        seq += 1
        if tick % 3 == 0:
            hex_msg2 = COAP_DEMO_HEX_LIST[(tick + 3) % len(COAP_DEMO_HEX_LIST)]
            lines.append(f"coap {hex_msg2}")
            seq += 1
        # every 5th tick also send a compact (no-spaces) variant
        if tick % 5 == 0:
            compact = COAP_DEMO_HEX_LIST[(tick + 7) % len(COAP_DEMO_HEX_LIST)].replace(' ', '')
            lines.append(f"coap-compact {compact}")
            seq += 1

    if tick % 5 == 0:
        lines.append("<wrn> " + _msg(src, tick, seq, "warning", f"{src} warning at tick {tick:03d}"))
        seq += 1

    if tick % 7 == 0:
        lines.append("<err> " + _msg(src, tick, seq, "error", f"{src} error at tick {tick:03d}"))
        seq += 1

    if tick % 9 == 0:
        # Deliberate duplicated source prefix for raw snippet cleanup tests.
        lines.append(f"[{src}] " + _msg(src, tick, seq, "prefix-cleanup", "duplicated source prefix"))
        seq += 1

    if tick % 11 == 0:
        # Deliberate embedded timestamp for raw snippet cleanup tests.
        lines.append(f"[{_embedded_timestamp(tick)}] " + _msg(src, tick, seq, "timestamp-cleanup", "duplicated timestamp prefix"))
        seq += 1

    if tick % 13 == 0:
        lines.append(_msg(src, tick, seq, "filter-alpha", "alpha filter target"))
        seq += 1

    if tick % 17 == 0:
        lines.append(_msg(src, tick, seq, "filter-beta", "beta filter target"))
        seq += 1

    return lines, seq


def build_cbor_records(src: str, tick: int, seq_start: int) -> tuple[list[dict], int]:
    """Return deterministic CBOR-encodable records for one source/tick.

    Each record is a dict that, when encoded as CBOR and decoded by
    CborDatagramParser, produces a line containing the same semantic
    tokens that UI tests assert on (e.g. ``kind=filter-alpha``, ``tick=11``).
    """
    seq = seq_start
    records: list[dict] = []

    records.append({
        "src": src,
        "tick": tick,
        "seq": seq,
        "kind": "sync",
        "msg": f"{src} synchronized step {tick:03d}",
    })
    seq += 1

    if tick % 5 == 0:
        records.append({
            "src": src,
            "tick": tick,
            "seq": seq,
            "kind": "warning",
            "msg": f"{src} warning at tick {tick:03d}",
        })
        seq += 1

    if tick % 7 == 0:
        records.append({
            "src": src,
            "tick": tick,
            "seq": seq,
            "kind": "error",
            "msg": f"{src} error at tick {tick:03d}",
        })
        seq += 1

    if tick % 9 == 0:
        records.append({
            "src": src,
            "tick": tick,
            "seq": seq,
            "kind": "prefix-cleanup",
            "msg": "duplicated source prefix",
        })
        seq += 1

    if tick % 11 == 0:
        records.append({
            "src": src,
            "tick": tick,
            "seq": seq,
            "kind": "timestamp-cleanup",
            "msg": "duplicated timestamp prefix",
        })
        seq += 1

    if tick % 13 == 0:
        records.append({
            "src": src,
            "tick": tick,
            "seq": seq,
            "kind": "filter-alpha",
            "msg": "alpha filter target",
        })
        seq += 1

    if tick % 17 == 0:
        records.append({
            "src": src,
            "tick": tick,
            "seq": seq,
            "kind": "filter-beta",
            "msg": "beta filter target",
        })
        seq += 1

    return records, seq


# ═══════════════════════════════════════════════════════════════════
# Curated content — REST API testing story
# ═══════════════════════════════════════════════════════════════════

CURATED_DEVICE_A: dict[int, list[str]] = {
    1: ["<inf> net: link up, MAC=de:ad:be:ef:01:02", "<inf> httpd: listening on 0.0.0.0:8080, awaiting commands"],
    2: ["<inf> net: connection from 192.168.1.100:45012", "<inf> httpd: accepted, 1 active session"],
    3: ["<inf> httpd: session established, protocol REST/1.0", "<inf> sys: test mode enabled via header X-Test-Suite"],
    4: ["<inf> httpd: << GET /api/status", f"<inf> coap rx: frame AA 55 payload {COAP_DEMO_HEX_LIST[0]}"],
    5: ["<inf> handler: processing status request", "<inf> handler: mem_free=18324KB uptime=3600s cpu_load=12%", "<inf> httpd: >> 200 OK  {status:ok, uptime:3600}  (12ms)"],
    6: ["<wrn> sys: cpu temperature at 87C — approaching throttle threshold"],
    7: ["<inf> httpd: << GET /api/config", "<inf> handler: reading config service endpoint"],
    8: ["<err> handler: config service not initialized", "<err> handler: config.json missing from flash partition"],
    9: ["<inf> httpd: >> 503 Service Unavailable  {error:config_unavailable}  (8ms)"],
    10: ["<err> sys: memory allocation failed at httpd_handler.c:412", "<err> sys: heap fragmented (max block 4096 bytes, requested 8192)"],
    11: ["<inf> httpd: << GET /api/health", "<inf> handler: health check requested"],
    12: ["<inf> handler: services=all_ok, last_boot=2026-05-31T10:28:04Z", "<inf> handler: self-test — passed (mem=18240KB, disk=ok)"],
    13: ["<inf> httpd: >> 200 OK  {status:healthy, uptime:3612}  (15ms)"],
    14: ["<inf> httpd: session close from 192.168.1.100:45012", "<inf> httpd: connection terminated, session count=0"],
    15: ["<inf> sys: test flag cleared, returning to normal mode"],
    16: ["<inf> httpd: listening, awaiting commands"],
}

CURATED_HOST: dict[int, list[str]] = {
    1: ["<inf> session: starting test suite — target: 192.168.1.10:8080", "<inf> session: test vector loaded — 3 test cases"],
    2: ["<inf> session: connecting to DEVICE_A...", "<inf> session: connected (TCP seq=1)"],
    3: ["<inf> session: handshake complete, protocol REST/1.0"],
    4: ["<inf> req: >> GET /api/status", "<inf> req: awaiting response..."],
    5: ["<inf> resp: << 200 OK  in 12ms", "<inf> resp: body={status:ok, uptime:3600, mem_free:18324KB}"],
    6: ["<inf> req: >> GET /api/config", "<inf> req: awaiting response..."],
    7: ["<inf> req: waiting... (2s timeout)"],
    8: ["<inf> resp: << 503 Service Unavailable  in 8ms", "<inf> resp: body={error:config_unavailable}", "<inf> assert: status=503 — expected failure confirmed"],
    9: ["<wrn> req: response time 2032ms exceeds SLA of 2000ms", "<err> session: connection reset by peer on retry, reconnecting..."],
    10: ["<inf> req: >> GET /api/health", "<inf> req: awaiting response..."],
    11: ["<inf> req: waiting..."],
    12: ["<inf> resp: << 200 OK  in 15ms", "<inf> resp: body={status:healthy, uptime:3612, self_test:passed}", "<inf> assert: status=healthy — OK"],
    13: ["<inf> session: all requests completed", "<inf> session: closing connection..."],
    14: ["<inf> session: disconnected", "<inf> session: test suite complete — 3/3 passed in 1.234s"],
    15: ["<inf> session: generating report..."],
    16: ["<inf> session: report written to test-results/2026-05-31.xml"],
}

CURATED_AUX: dict[int, list[str]] = {
    1: ["<inf> mon: sensor online, fw v1.9.2", "<inf> mon: monitoring ambient at 1s interval"],
    2: ["<inf> mon: ambient temp=23.4C noise=-72dBm"],
    3: ["<inf> mon: heartbeat OK"],
    4: ["<inf> mon: ambient stable — no anomalies"],
    5: ["<inf> mon: ambient temp=23.4C noise=-71dBm"],
    6: ["<inf> mon: ambient temp=23.5C noise=-72dBm"],
    7: ["<inf> mon: heartbeat OK"],
    8: ["<inf> mon: cross-check request received from DEVICE_A", "<inf> mon: confirming — ambient nominal, no interference"],
    9: ["<inf> mon: ambient temp=23.4C noise=-72dBm"],
    10: ["<inf> mon: heartbeat OK"],
    11: ["<inf> mon: ambient temp=23.4C noise=-72dBm"],
    12: ["<inf> mon: ambient temp=23.5C noise=-71dBm"],
    13: ["<inf> mon: heartbeat OK — all clear"],
    14: ["<inf> mon: ambient temp=23.4C noise=-72dBm"],
    15: ["<inf> mon: device under test activity complete"],
    16: ["<inf> mon: continuing normal monitoring"],
}

CURATED_PYTEST: dict[int, list[str]] = {
    1: ["[STEP] test_suite_init — loading test vectors...", "[STEP] test_suite_init — fixture setup: http_client, device_session"],
    2: ["[STEP] test_suite_init — connecting to DEVICE_A@192.168.1.10:8080", "[STEP] ✓ setup fixture 'device_session' — connected"],
    3: ["[PASS] test_01_get_status — test case started"],
    4: ["[STEP] test_01_get_status — sending GET /api/status", "[STEP] test_01_get_status — awaiting response"],
    5: ["[STEP] test_01_get_status — assert response.status == 200", "[STEP] test_01_get_status — assert response.body.status == 'ok'", "[PASS] ✓ test_01_get_status — PASSED (0.342s)"],
    6: ["[PASS] test_02_get_config — test case started", "[STEP] test_02_get_config — sending GET /api/config", "[STEP] test_02_get_config — expecting: 503 Service Unavailable"],
    7: ["[STEP] test_02_get_config — awaiting response"],
    8: ["[STEP] test_02_get_config — assert response.status == 503", "[STEP] test_02_get_config — assert response.body.error == 'config_unavailable'", "[PASS] ✓ test_02_get_config — PASSED (0.156s)", "[STEP] ✓ expected failure confirmed — error handling verified"],
    9: [],
    10: ["[PASS] test_03_get_health — test case started", "[STEP] test_03_get_health — sending GET /api/health"],
    11: ["[STEP] test_03_get_health — awaiting response"],
    12: ["[STEP] test_03_get_health — assert response.status == 200", "[STEP] test_03_get_health — assert response.body.status == 'healthy'", "[PASS] ✓ test_03_get_health — PASSED (0.089s)"],
    13: ["[STEP] test_suite_teardown — closing device session", "[STEP] ✓ fixture 'device_session' — disconnected cleanly"],
    14: ["[PASS] ========== 3 passed in 1.234s =========="],
    15: ["<inf> test report: tests=3 passed=3 failed=0 duration=1.234s"],
    16: ["<inf> test report: written to test-results/2026-05-31.xml"],
}

CURATED_CBOR: dict[int, list[dict]] = {
    1: [{"src": "DIAG", "kind": "sync", "state": "INIT", "msg": "diagnostic channel ready"}, {"src": "DIAG", "kind": "test_suite", "name": "device_api_test", "version": "2.1.0"}],
    2: [{"src": "DIAG", "kind": "connection", "src_host": "HOST", "dst_host": "DEVICE_A", "status": "connected"}],
    3: [{"src": "DIAG", "kind": "test_case", "name": "test_01_get_status", "method": "GET", "path": "/api/status"}],
    4: [{"src": "DIAG", "kind": "request", "method": "GET", "path": "/api/status", "seq": 1}],
    5: [{"src": "DIAG", "kind": "response", "method": "GET", "path": "/api/status", "status": 200, "duration_ms": 12}, {"src": "DIAG", "kind": "test_result", "name": "test_01_get_status", "result": "PASSED", "duration_ms": 342}],
    6: [{"src": "DIAG", "kind": "test_case", "name": "test_02_get_config", "method": "GET", "path": "/api/config", "expected_status": 503}],
    7: [{"src": "DIAG", "kind": "request", "method": "GET", "path": "/api/config", "seq": 2}],
    8: [{"src": "DIAG", "kind": "response", "method": "GET", "path": "/api/config", "status": 503, "error": "config_unavailable", "duration_ms": 8}, {"src": "DIAG", "kind": "test_result", "name": "test_02_get_config", "result": "PASSED", "expected_failure": True, "duration_ms": 156}],
    9: [],
    10: [{"src": "DIAG", "kind": "test_case", "name": "test_03_get_health", "method": "GET", "path": "/api/health"}],
    11: [{"src": "DIAG", "kind": "request", "method": "GET", "path": "/api/health", "seq": 3}],
    12: [{"src": "DIAG", "kind": "response", "method": "GET", "path": "/api/health", "status": 200, "duration_ms": 15}, {"src": "DIAG", "kind": "test_result", "name": "test_03_get_health", "result": "PASSED", "duration_ms": 89}],
    13: [{"src": "DIAG", "kind": "connection", "src_host": "HOST", "dst_host": "DEVICE_A", "status": "disconnected"}],
    14: [{"src": "DIAG", "kind": "summary", "tests": 3, "passed": 3, "failed": 0, "duration_ms": 1234}],
    15: [{"src": "DIAG", "kind": "sync", "state": "COMPLETE"}],
    16: [{"src": "DIAG", "kind": "sync", "state": "IDLE"}],
}

# Source name → content dict mapping for curated mode
CURATED_LINES: dict[str, dict[int, list[str]]] = {
    "SENSOR_A": CURATED_DEVICE_A,
    "SENSOR_B": CURATED_HOST,
    "SENSOR_C": CURATED_AUX,
    "SENSOR_D": CURATED_PYTEST,
}

CURATED_MARKERS: list[tuple[int, str, str, str]] = [
    (1,  "*",       "Session started — {name} online", "green"),
    (3,  "SENSOR_D", "test_01_get_status — starting", "cyan"),
    (3,  "SENSOR_B", "GET /api/status — request sent", "cyan"),
    (5,  "SENSOR_A", "GET /api/status — 200 OK answered", "green"),
    (5,  "SENSOR_D", "✓ test_01_get_status PASSED", "green"),
    (6,  "SENSOR_D", "test_02_get_config — starting", "cyan"),
    (6,  "SENSOR_B", "GET /api/config — request sent", "cyan"),
    (8,  "SENSOR_A", "GET /api/config — 503 answered (expected error)", "yellow"),
    (8,  "SENSOR_D", "✓ test_02_get_config PASSED (expected failure)", "green"),
    (9,  "SENSOR_C", "Cross-check — ambient nominal during test", "cyan"),
    (10, "SENSOR_D", "test_03_get_health — starting", "cyan"),
    (10, "SENSOR_B", "GET /api/health — request sent", "cyan"),
    (12, "SENSOR_A", "GET /api/health — 200 OK answered", "green"),
    (12, "SENSOR_D", "✓ test_03_get_health PASSED", "green"),
    (14, "*",       "Test suite complete — {name} done", "green"),
]

def build_curated_udp_lines(src: str, tick: int) -> list[str]:
    """Return pre-defined lines for one source/tick in curated mode."""
    src_lines = CURATED_LINES.get(src, {})
    return src_lines.get(tick, [])

def build_curated_cbor_records(tick: int) -> list[dict]:
    """Return pre-defined CBOR records for one tick in curated mode."""
    return CURATED_CBOR.get(tick, [])


class InjectFanout:
    def __init__(self, targets: list[Target], connect_timeout: float, source: str = "TEST"):
        self._clients: dict[str, LogClient] = {}
        self._connect_timeout = connect_timeout
        for t in targets:
            client = LogClient(t.host, t.port, source=source, connect_timeout=connect_timeout)
            client.connect()
            self._clients[t.name] = client
            print(f"[det-demo] connected inject {t.name} at {t.host}:{t.port}")

    def marker(self, src: str, message: str, *, color: Optional[str] = None) -> None:
        client = self._clients.get(src)
        if client is not None:
            client.marker(message, color=color)

    def close(self) -> None:
        for client in self._clients.values():
            client.close()
        self._clients.clear()


def run(args: argparse.Namespace) -> int:
    if not args.udp:
        raise ValueError("at least one --udp target is required")
    if args.tick_ms <= 0:
        raise ValueError("--tick-ms must be > 0")
    if args.cycles < 0:
        raise ValueError("--cycles must be >= 0")

    is_curated = args.content == "curated"
    max_curated_tick = max(
        max((lines or {0: []}).keys())
        for lines in CURATED_LINES.values()
    ) if is_curated else 0
    mode_label = "curated" if is_curated else "test"

    print(f"[det-demo] content={mode_label} UDP targets:")
    for t in args.udp:
        print(f"  - {t.name} -> {t.host}:{t.port}")
    if args.inject:
        print(f"[det-demo] content={mode_label} inject targets:")
        for t in args.inject:
            print(f"  - {t.name} -> {t.host}:{t.port}")
    print(f"[det-demo] tick_ms={args.tick_ms:g} cycles={'infinite' if args.cycles == 0 else args.cycles}{' mode=CBOR' if args.cbor else ''}")

    inject_source = "DEMO" if is_curated else "TEST"
    seq_by_src = {t.name: 1 for t in args.udp}
    inject = InjectFanout(args.inject, args.connect_timeout, source=inject_source) if args.inject else None
    tick_interval = args.tick_ms / 1000.0
    per_source_offset = min(0.005, tick_interval / max(1, len(args.udp) * 4))

    tick = 0
    next_tick_at = time.monotonic()

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp_sock:
        try:
            while args.cycles == 0 or tick < args.cycles:
                tick += 1
                # In infinite curated mode, wrap tick so content cycles forever
                if args.cycles == 0 and is_curated and max_curated_tick > 0 and tick > max_curated_tick:
                    tick = 1
                now = time.monotonic()
                if now < next_tick_at:
                    time.sleep(next_tick_at - now)

                for index, target in enumerate(args.udp):
                    if index > 0 and per_source_offset > 0:
                        time.sleep(per_source_offset)

                    if is_curated:
                        # Curated mode: look up pre-defined content
                        if args.cbor:
                            records = build_curated_cbor_records(tick)
                            for rec in records:
                                udp_sock.sendto(cbor2.dumps(rec), (target.host, target.port))
                        else:
                            lines = build_curated_udp_lines(target.name, tick)
                            if lines:
                                payload = ("\n".join(lines) + "\n").encode("utf-8")
                                udp_sock.sendto(payload, (target.host, target.port))
                    else:
                        # Test mode: generate on-the-fly with tick%N patterns
                        if args.cbor:
                            records, next_seq = build_cbor_records(target.name, tick, seq_by_src[target.name])
                            seq_by_src[target.name] = next_seq
                            for rec in records:
                                udp_sock.sendto(cbor2.dumps(rec), (target.host, target.port))
                        else:
                            lines, next_seq = build_udp_lines(target.name, tick, seq_by_src[target.name])
                            seq_by_src[target.name] = next_seq
                            payload = ("\n".join(lines) + "\n").encode("utf-8")
                            udp_sock.sendto(payload, (target.host, target.port))

                # Inject markers
                if inject is not None:
                    if is_curated:
                        for m_tick, m_src, m_msg, m_color in CURATED_MARKERS:
                            if m_tick == tick:
                                if m_src == "*":
                                    for target in args.udp:
                                        client = inject._clients.get(target.name)
                                        if client:
                                            client.marker(m_msg.format(name=target.name), color=m_color)
                                else:
                                    client = inject._clients.get(m_src)
                                    if client:
                                        client.marker(m_msg, color=m_color)
                    else:
                        if tick % 10 == 0:
                            for target in args.udp:
                                seq = seq_by_src[target.name]
                                seq_by_src[target.name] = seq + 1
                                inject.marker(
                                    target.name,
                                    _msg(target.name, tick, seq, "inject", f"inject marker for {target.name}"),
                                    color="cyan",
                                )

                if tick == 1 or tick % 25 == 0:
                    print(f"[det-demo] sent tick={tick:03d}")
                next_tick_at += tick_interval
        except KeyboardInterrupt:
            print("\n[det-demo] interrupted")
        finally:
            if inject is not None:
                inject.close()

    print(f"[det-demo] done at tick={tick:03d}")
    return 0


def main() -> int:
    args = parse_args()
    try:
        return run(args)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
