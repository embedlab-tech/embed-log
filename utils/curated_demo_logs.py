#!/usr/bin/env python3
"""
Curated demo log generator for the embed-log website demo.

Generates realistic embedded-device REST API testing logs across
all sources, telling a coherent test-investigation story.

Layout (embed-log.curated-demo.yml):
  DevA    → SENSOR_A (DEVICE_A) + SENSOR_B (HOST)
  DevB    → SENSOR_C (AUX)
  PYTEST  → SENSOR_D (PYTEST)
  cbor-tab → SENSOR_CBOR (CBOR)

Story: REST API testing of an embedded device (DEVICE_A).
  Phase 1 — Test setup                  (ticks 1-3)
  Phase 2 — GET /api/status → OK       (ticks 4-9)
  Phase 3 — GET /api/config → 503      (ticks 10-16)
  Phase 4 — GET /api/health → OK      (ticks 17-22)
  Phase 5 — Test complete              (ticks 23-25)

Usage:
    python3 utils/curated_demo_logs.py [--tick-ms TICK] [--output-dir DIR]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

import cbor2

ROOT_DIR = Path(__file__).resolve().parents[1]
if str(ROOT_DIR) not in sys.path:
    sys.path.insert(0, str(ROOT_DIR))

from backend.log_client import LogClient

# ── Ports matching embed-log.curated-demo.yml ──
UDP_PORTS = {"SENSOR_A": 6000, "SENSOR_B": 6001, "SENSOR_C": 6002, "SENSOR_D": 6004, "SENSOR_CBOR": 6003}
INJECT_PORTS = {"SENSOR_A": 5001, "SENSOR_B": 5002, "SENSOR_C": 5003, "SENSOR_D": 5004}
WS_PORT = 8080

# ═══════════════════════════════════════════════════════════════════
# SENSOR_A / DEVICE_A — Device Under Test
# Shows incoming requests and responses
# ═══════════════════════════════════════════════════════════════════
DEVICE_A_LOG: dict[int, list[str]] = {
    1: [
        "[INFO] net: link up, MAC=de:ad:be:ef:01:02",
        "[INFO] httpd: listening on 0.0.0.0:8080, awaiting commands",
    ],
    2: [
        "[INFO] net: connection from 192.168.1.100:45012",
        "[INFO] httpd: accepted, 1 active session",
    ],
    3: [
        "[INFO] httpd: session established, protocol REST/1.0",
        "[INFO] sys: test mode enabled via header X-Test-Suite",
    ],
    4: [
        "[INFO] httpd: << GET /api/status",
        "[INFO] handler: processing status request",
    ],
    5: [
        "[INFO] handler: mem_free=18324KB uptime=3600s cpu_load=12%",
        "[INFO] httpd: >> 200 OK  {status:ok, uptime:3600}  (12ms)",
    ],
    6: [],
    7: [
        "[INFO] httpd: << GET /api/config",
        "[INFO] handler: reading config service endpoint",
    ],
    8: [
        "[ERR] handler: config service not initialized",
        "[ERR] handler: config.json missing from flash partition",
    ],
    9: [
        "[INFO] httpd: >> 503 Service Unavailable  {error:config_unavailable}  (8ms)",
    ],
    10: [],
    11: [
        "[INFO] httpd: << GET /api/health",
        "[INFO] handler: health check requested",
    ],
    12: [
        "[INFO] handler: services=all_ok, last_boot=2026-05-31T10:28:04Z",
        "[INFO] handler: self-test — passed (mem=18240KB, disk=ok)",
    ],
    13: [
        "[INFO] httpd: >> 200 OK  {status:healthy, uptime:3612}  (15ms)",
    ],
    14: [
        "[INFO] httpd: session close from 192.168.1.100:45012",
        "[INFO] httpd: connection terminated, session count=0",
    ],
    15: [
        "[INFO] sys: test flag cleared, returning to normal mode",
    ],
    16: [
        "[INFO] httpd: listening, awaiting commands",
    ],
}

# ═══════════════════════════════════════════════════════════════════
# SENSOR_B / HOST — Test Controller / Workstation
# Shows requests sent and responses received
# ═══════════════════════════════════════════════════════════════════
HOST_LOG: dict[int, list[str]] = {
    1: [
        "[INFO] session: starting test suite — target: 192.168.1.10:8080",
        "[INFO] session: test vector loaded — 3 test cases",
    ],
    2: [
        "[INFO] session: connecting to DEVICE_A...",
        "[INFO] session: connected (TCP seq=1)",
    ],
    3: [
        "[INFO] session: handshake complete, protocol REST/1.0",
    ],
    4: [
        "[INFO] req: >> GET /api/status",
        "[INFO] req: awaiting response...",
    ],
    5: [
        "[INFO] resp: << 200 OK  in 12ms",
        "[INFO] resp: body={status:ok, uptime:3600, mem_free:18324KB}",
    ],
    6: [
        "[INFO] req: >> GET /api/config",
        "[INFO] req: awaiting response...",
    ],
    7: [
        "[INFO] req: waiting... (2s timeout)",
    ],
    8: [
        "[INFO] resp: << 503 Service Unavailable  in 8ms",
        "[INFO] resp: body={error:config_unavailable}",
        "[INFO] assert: status=503 — expected failure confirmed",
    ],
    9: [],
    10: [
        "[INFO] req: >> GET /api/health",
        "[INFO] req: awaiting response...",
    ],
    11: [
        "[INFO] req: waiting...",
    ],
    12: [
        "[INFO] resp: << 200 OK  in 15ms",
        "[INFO] resp: body={status:healthy, uptime:3612, self_test:passed}",
        "[INFO] assert: status=healthy — OK",
    ],
    13: [
        "[INFO] session: all requests completed",
        "[INFO] session: closing connection...",
    ],
    14: [
        "[INFO] session: disconnected",
        "[INFO] session: test suite complete — 3/3 passed in 1.234s",
    ],
    15: [
        "[INFO] session: generating report...",
    ],
    16: [
        "[INFO] session: report written to test-results/2026-05-31.xml",
    ],
}

# ═══════════════════════════════════════════════════════════════════
# SENSOR_C / AUX — Auxiliary monitoring device
# Background ambient readings corroborating the timeline
# ═══════════════════════════════════════════════════════════════════
AUX_LOG: dict[int, list[str]] = {
    1: [
        "[INFO] mon: sensor online, fw v1.9.2",
        "[INFO] mon: monitoring ambient at 1s interval",
    ],
    2: [
        "[INFO] mon: ambient temp=23.4C noise=-72dBm",
    ],
    3: [
        "[INFO] mon: heartbeat OK",
    ],
    4: [
        "[INFO] mon: ambient stable — no anomalies",
    ],
    5: [
        "[INFO] mon: ambient temp=23.4C noise=-71dBm",
    ],
    6: [
        "[INFO] mon: ambient temp=23.5C noise=-72dBm",
    ],
    7: [
        "[INFO] mon: heartbeat OK",
    ],
    8: [
        "[INFO] mon: cross-check request received from DEVICE_A",
        "[INFO] mon: confirming — ambient nominal, no interference",
    ],
    9: [
        "[INFO] mon: ambient temp=23.4C noise=-72dBm",
    ],
    10: [
        "[INFO] mon: heartbeat OK",
    ],
    11: [
        "[INFO] mon: ambient temp=23.4C noise=-72dBm",
    ],
    12: [
        "[INFO] mon: ambient temp=23.5C noise=-71dBm",
    ],
    13: [
        "[INFO] mon: heartbeat OK — all clear",
    ],
    14: [
        "[INFO] mon: ambient temp=23.4C noise=-72dBm",
    ],
    15: [
        "[INFO] mon: device under test activity complete",
    ],
    16: [
        "[INFO] mon: continuing normal monitoring",
    ],
}

# ═══════════════════════════════════════════════════════════════════
# SENSOR_D / PYTEST — Test execution log
# Shows test steps, assertions, and results
# ═══════════════════════════════════════════════════════════════════
PYTEST_LOG: dict[int, list[str]] = {
    1: [
        "[STEP] test_suite_init — loading test vectors...",
        "[STEP] test_suite_init — fixture setup: http_client, device_session",
    ],
    2: [
        "[STEP] test_suite_init — connecting to DEVICE_A@192.168.1.10:8080",
        "[STEP] ✓ setup fixture 'device_session' — connected",
    ],
    3: [
        "[PASS] test_01_get_status — test case started",
    ],
    4: [
        "[STEP] test_01_get_status — sending GET /api/status",
        "[STEP] test_01_get_status — awaiting response",
    ],
    5: [
        "[STEP] test_01_get_status — assert response.status == 200",
        "[STEP] test_01_get_status — assert response.body.status == 'ok'",
        "[PASS] ✓ test_01_get_status — PASSED (0.342s)",
    ],
    6: [
        "[PASS] test_02_get_config — test case started",
        "[STEP] test_02_get_config — sending GET /api/config",
        "[STEP] test_02_get_config — expecting: 503 Service Unavailable",
    ],
    7: [
        "[STEP] test_02_get_config — awaiting response",
    ],
    8: [
        "[STEP] test_02_get_config — assert response.status == 503",
        "[STEP] test_02_get_config — assert response.body.error == 'config_unavailable'",
        "[PASS] ✓ test_02_get_config — PASSED (0.156s)",
        "[STEP] ✓ expected failure confirmed — error handling verified",
    ],
    9: [],
    10: [
        "[PASS] test_03_get_health — test case started",
        "[STEP] test_03_get_health — sending GET /api/health",
    ],
    11: [
        "[STEP] test_03_get_health — awaiting response",
    ],
    12: [
        "[STEP] test_03_get_health — assert response.status == 200",
        "[STEP] test_03_get_health — assert response.body.status == 'healthy'",
        "[PASS] ✓ test_03_get_health — PASSED (0.089s)",
    ],
    13: [
        "[STEP] test_suite_teardown — closing device session",
        "[STEP] ✓ fixture 'device_session' — disconnected cleanly",
    ],
    14: [
        "[PASS] ========== 3 passed in 1.234s ==========",
    ],
    15: [
        "[INFO] test report: tests=3 passed=3 failed=0 duration=1.234s",
    ],
    16: [
        "[INFO] test report: written to test-results/2026-05-31.xml",
    ],
}

# ═══════════════════════════════════════════════════════════════════
# SENSOR_CBOR / CBOR — Structured diagnostic channel
# ═══════════════════════════════════════════════════════════════════
CBOR_LOG: dict[int, list[dict]] = {
    1: [
        {"src": "DIAG", "kind": "sync", "state": "INIT", "msg": "diagnostic channel ready"},
        {"src": "DIAG", "kind": "test_suite", "name": "device_api_test", "version": "2.1.0"},
    ],
    2: [
        {"src": "DIAG", "kind": "connection", "src_host": "HOST", "dst_host": "DEVICE_A", "status": "connected"},
    ],
    3: [
        {"src": "DIAG", "kind": "test_case", "name": "test_01_get_status", "method": "GET", "path": "/api/status"},
    ],
    4: [
        {"src": "DIAG", "kind": "request", "method": "GET", "path": "/api/status", "seq": 1},
    ],
    5: [
        {"src": "DIAG", "kind": "response", "method": "GET", "path": "/api/status", "status": 200, "duration_ms": 12},
        {"src": "DIAG", "kind": "test_result", "name": "test_01_get_status", "result": "PASSED", "duration_ms": 342},
    ],
    6: [
        {"src": "DIAG", "kind": "test_case", "name": "test_02_get_config", "method": "GET", "path": "/api/config", "expected_status": 503},
    ],
    7: [
        {"src": "DIAG", "kind": "request", "method": "GET", "path": "/api/config", "seq": 2},
    ],
    8: [
        {"src": "DIAG", "kind": "response", "method": "GET", "path": "/api/config", "status": 503, "error": "config_unavailable", "duration_ms": 8},
        {"src": "DIAG", "kind": "test_result", "name": "test_02_get_config", "result": "PASSED", "expected_failure": True, "duration_ms": 156},
    ],
    9: [],
    10: [
        {"src": "DIAG", "kind": "test_case", "name": "test_03_get_health", "method": "GET", "path": "/api/health"},
    ],
    11: [
        {"src": "DIAG", "kind": "request", "method": "GET", "path": "/api/health", "seq": 3},
    ],
    12: [
        {"src": "DIAG", "kind": "response", "method": "GET", "path": "/api/health", "status": 200, "duration_ms": 15},
        {"src": "DIAG", "kind": "test_result", "name": "test_03_get_health", "result": "PASSED", "duration_ms": 89},
    ],
    13: [
        {"src": "DIAG", "kind": "connection", "src_host": "HOST", "dst_host": "DEVICE_A", "status": "disconnected"},
    ],
    14: [
        {"src": "DIAG", "kind": "summary", "tests": 3, "passed": 3, "failed": 0, "duration_ms": 1234},
    ],
    15: [
        {"src": "DIAG", "kind": "sync", "state": "COMPLETE"},
    ],
    16: [
        {"src": "DIAG", "kind": "sync", "state": "IDLE"},
    ],
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate curated demo logs for the embed-log website demo."
    )
    parser.add_argument(
        "--tick-ms", type=float, default=300.0,
        help="Milliseconds between ticks (default: 300).",
    )
    parser.add_argument(
        "--output-dir", type=str, default=None,
        help="Directory to copy the exported session.html into.",
    )
    parser.add_argument(
        "--no-serve", action="store_true",
        help="Skip starting the server (assume it is already running).",
    )
    parser.add_argument(
        "--no-export", action="store_true",
        help="Skip the export step (just generate traffic).",
    )
    return parser.parse_args()


def _free_port(port: int, proto: str = "tcp") -> None:
    """Check and free a port if in use."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM if proto == "tcp" else socket.SOCK_DGRAM) as s:
        if proto == "tcp":
            in_use = s.connect_ex(("127.0.0.1", port)) == 0
        else:
            try:
                s.bind(("127.0.0.1", port))
                in_use = False
            except OSError:
                in_use = True
    if not in_use:
        return
    try:
        out = subprocess.run(
            ["lsof", "-tiTCP", str(port), "-sTCP:LISTEN"],
            capture_output=True, text=True, timeout=5,
        )
        pids = [int(p) for p in out.stdout.strip().split() if p.strip().isdigit()]
        for pid in pids:
            os.kill(pid, signal.SIGTERM)
            time.sleep(0.3)
    except (FileNotFoundError, subprocess.TimeoutExpired, ProcessLookupError):
        pass


def send_udp(sock: socket.socket, port: int, lines: list[str]) -> None:
    """Send one or more lines as a single UDP datagram."""
    if not lines:
        return
    payload = ("\n".join(lines) + "\n").encode("utf-8")
    sock.sendto(payload, ("127.0.0.1", port))


def send_cbor(sock: socket.socket, port: int, records: list[dict]) -> None:
    """Send each CBOR record as a separate UDP datagram."""
    for rec in records:
        sock.sendto(cbor2.dumps(rec), ("127.0.0.1", port))


def inject_marker(client: LogClient, message: str, color: str = "cyan") -> None:
    """Send a coloured marker via the inject port."""
    client.marker(message, color=color)


def export_session(log_dir: Path, session_id: str, output_path: Path) -> bool:
    """Export a session to HTML by calling merge_logs.py directly."""
    from backend.cli.util import read_manifest, read_session_dir

    sdir = read_session_dir(log_dir, session_id)
    if not sdir:
        print(f"ERROR: session directory not found: {session_id}", file=sys.stderr)
        return False

    manifest = read_manifest(sdir)
    if not manifest:
        print(f"ERROR: no manifest for {session_id}", file=sys.stderr)
        return False

    source_files = manifest.get("source_files", {})
    tabs = manifest.get("tabs", [])
    pane_labels = manifest.get("pane_labels") or {}
    timestamp_mode = str(manifest.get("timestamp_mode") or "absolute")
    first_log_at = manifest.get("first_log_at")

    merge_script = ROOT_DIR / "utils" / "merge_logs.py"
    if not merge_script.is_file():
        print(f"ERROR: merge script not found: {merge_script}", file=sys.stderr)
        return False

    cmd = [sys.executable, str(merge_script)]
    if timestamp_mode:
        cmd.extend(["--timestamp-mode", timestamp_mode])
    if first_log_at:
        cmd.extend(["--first-log-at", first_log_at])

    for tab in tabs:
        label = tab["label"]
        cmd.extend(["--tab", label])
        tab_pane_labels = tab.get("pane_labels", {})
        for pane in tab.get("panes", []):
            file_path = source_files.get(pane)
            if not file_path:
                print(f"  WARNING: no source file for pane {pane!r}", file=sys.stderr)
                continue
            pane_label = tab_pane_labels.get(pane, pane_labels.get(pane, pane))
            abs_path = (ROOT_DIR / file_path).resolve()
            cmd.extend([f"{pane}={pane_label}", str(abs_path)])

    cmd.extend(["--output", str(output_path.resolve())])

    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT_DIR)
        print(proc.stdout, end="")
        if proc.returncode != 0:
            print(f"ERROR: merge failed: {proc.stderr.strip()}", file=sys.stderr)
            return False
    except Exception as exc:
        print(f"ERROR: merge failed: {exc}", file=sys.stderr)
        return False

    print(f"Exported: {output_path}")
    return True


def main() -> int:
    args = parse_args()

    tick_interval = args.tick_ms / 1000.0
    log_dir = ROOT_DIR / "logs"
    output_path = Path(args.output_dir) / "session.html" if args.output_dir else ROOT_DIR / "demo-session.html"

    config_path = ROOT_DIR / "embed-log.curated-demo.yml"
    if not config_path.is_file():
        print(f"ERROR: curated demo config not found: {config_path}", file=sys.stderr)
        return 1

    server = None

    if not args.no_serve:
        print("=== embed-log curated demo (REST API testing story) ===")
        print("")

        print("Checking ports...")
        for port in set(UDP_PORTS.values()) | set(INJECT_PORTS.values()) | {WS_PORT}:
            proto = "udp" if port in {6000, 6001, 6002, 6003, 6004} else "tcp"
            _free_port(port, proto)

        python = sys.executable
        server_cmd = [
            python, str(ROOT_DIR / "backend" / "server.py"),
            "run",
            "--config", str(config_path),
            "--ws-port", str(WS_PORT),
            "--no-open-browser",
        ]
        print(f"Starting embed-log server with curated config on port {WS_PORT}...")
        server = subprocess.Popen(
            server_cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        time.sleep(2)
        if server.poll() is not None:
            print("ERROR: server failed to start.", file=sys.stderr)
            return 1
        print("Server started.")
    else:
        print("Skipping server start (--no-serve).")
        time.sleep(1)

    # ── Connect inject clients ──
    inject_clients: dict[str, LogClient] = {}
    for name, port in INJECT_PORTS.items():
        try:
            client = LogClient("127.0.0.1", port, source="DEMO", connect_timeout=15)
            client.connect()
            inject_clients[name] = client
            print(f"  inject connected: {name} -> {port}")
        except ConnectionRefusedError:
            print(f"  WARNING: inject {name} -> {port} refused", file=sys.stderr)

    # ── Generate curated traffic ──
    print(f"\nGenerating curated logs (tick={args.tick_ms:.0f}ms)...")
    max_tick = max(
        max(DEVICE_A_LOG.keys()),
        max(HOST_LOG.keys()),
        max(AUX_LOG.keys()),
        max(PYTEST_LOG.keys()),
        max(CBOR_LOG.keys()),
    )

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp_sock:
        for tick in range(1, max_tick + 1):
            send_udp(udp_sock, UDP_PORTS["SENSOR_A"], DEVICE_A_LOG.get(tick, []))
            send_udp(udp_sock, UDP_PORTS["SENSOR_B"], HOST_LOG.get(tick, []))
            send_udp(udp_sock, UDP_PORTS["SENSOR_C"], AUX_LOG.get(tick, []))
            send_udp(udp_sock, UDP_PORTS["SENSOR_D"], PYTEST_LOG.get(tick, []))
            send_cbor(udp_sock, UDP_PORTS["SENSOR_CBOR"], CBOR_LOG.get(tick, []))

            # ── Inject markers at key moments ──
            if tick == 1:
                for name in inject_clients:
                    inject_marker(inject_clients[name], f"Session started — {name} online", "green")

            if tick == 3:
                c = inject_clients.get("SENSOR_D")
                if c:
                    inject_marker(c, "test_01_get_status — starting", "cyan")
                c = inject_clients.get("SENSOR_B")
                if c:
                    inject_marker(c, "GET /api/status — request sent", "cyan")

            if tick == 5:
                c = inject_clients.get("SENSOR_A")
                if c:
                    inject_marker(c, "GET /api/status — 200 OK answered", "green")
                c = inject_clients.get("SENSOR_D")
                if c:
                    inject_marker(c, "✓ test_01_get_status PASSED", "green")

            if tick == 6:
                c = inject_clients.get("SENSOR_D")
                if c:
                    inject_marker(c, "test_02_get_config — starting", "cyan")
                c = inject_clients.get("SENSOR_B")
                if c:
                    inject_marker(c, "GET /api/config — request sent", "cyan")

            if tick == 8:
                c = inject_clients.get("SENSOR_A")
                if c:
                    inject_marker(c, "GET /api/config — 503 answered (expected error)", "yellow")
                c = inject_clients.get("SENSOR_D")
                if c:
                    inject_marker(c, "✓ test_02_get_config PASSED (expected failure)", "green")

            if tick == 9:
                c = inject_clients.get("SENSOR_C")
                if c:
                    inject_marker(c, "Cross-check — ambient nominal during test", "cyan")

            if tick == 10:
                c = inject_clients.get("SENSOR_D")
                if c:
                    inject_marker(c, "test_03_get_health — starting", "cyan")
                c = inject_clients.get("SENSOR_B")
                if c:
                    inject_marker(c, "GET /api/health — request sent", "cyan")

            if tick == 12:
                c = inject_clients.get("SENSOR_A")
                if c:
                    inject_marker(c, "GET /api/health — 200 OK answered", "green")
                c = inject_clients.get("SENSOR_D")
                if c:
                    inject_marker(c, "✓ test_03_get_health PASSED", "green")

            if tick == 14:
                for name, c in inject_clients.items():
                    inject_marker(c, f"Test suite complete — {name} done", "green")

            if tick == 1 or tick % 5 == 0:
                print(f"  tick={tick:03d}/{max_tick:03d}")

            if tick < max_tick:
                time.sleep(tick_interval)

    # ── Close inject clients ──
    for c in inject_clients.values():
        c.close()

    # ── Stop the server (triggers flush and auto-export) ──
    if server is not None:
        print("\nShutting down server...")
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()
        print("Server stopped.")

    if args.no_export:
        print("Done.")
        return 0

    # ── Copy server's auto-exported session.html ──
    print("\n=== Exporting session ===")

    if not log_dir.is_dir():
        print(f"ERROR: log directory not found: {log_dir}", file=sys.stderr)
        return 1

    sessions = sorted(
        [d for d in log_dir.iterdir() if d.is_dir() and (d / "manifest.json").is_file()],
        key=lambda d: d.stat().st_mtime,
        reverse=True,
    )
    if not sessions:
        print("ERROR: no sessions found in log directory.", file=sys.stderr)
        return 1

    session_id = sessions[0].name
    sdir = sessions[0]
    print(f"Session: {session_id}")

    auto_exported = sdir / "session.html"
    if not auto_exported.is_file():
        # Fallback: try to export manually
        print("No auto-exported session.html, re-exporting via merge_logs...")
        ok = export_session(log_dir, session_id, output_path)
        if not ok:
            return 1
    else:
        shutil.copy2(str(auto_exported), str(output_path))
        print(f"Copied: {auto_exported} -> {output_path}")

        # Verify content
        try:
            html_content = output_path.read_text(encoding="utf-8")
            if "var _logData = " not in html_content:
                print(f"WARNING: exported HTML has no log data. Re-exporting...", file=sys.stderr)
                ok = export_session(log_dir, session_id, output_path)
                if not ok:
                    return 1
        except Exception:
            pass

    print(f"\nDemo session exported to: {output_path}")
    print(f"Session ID: {session_id}")
    print("\n=== Curated demo complete ===")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
