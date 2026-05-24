#!/usr/bin/env python3
"""
Deterministic demo traffic generator for embed-log UI/E2E tests.

This tool sends predictable UDP log lines and optional inject markers to the
standard demo sources. It is intended for Playwright tests where random demo
traffic would make assertions flaky.

Example:
    python utils/deterministic_demo_traffic.py \
        --udp SENSOR_A=127.0.0.1:6000 \
        --udp SENSOR_B=127.0.0.1:6001 \
        --udp SENSOR_C=127.0.0.1:6002 \
        --inject SENSOR_A=127.0.0.1:5001 \
        --inject SENSOR_B=127.0.0.1:5002 \
        --inject SENSOR_C=127.0.0.1:5003 \
        --tick-ms 100 \
        --cycles 0
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
    return parser.parse_args()


def _msg(src: str, tick: int, seq: int, kind: str, message: str) -> str:
    return f'TEST src={src} tick={tick:03d} seq={seq:04d} kind={kind} msg="{message}"'


def _embedded_timestamp(tick: int) -> str:
    # Fixed timestamp is intentional: tests can verify cleanup of duplicated
    # payload timestamps without relying on wall-clock time.
    base = datetime(2026, 1, 1, 0, 0, 0, tzinfo=timezone.utc)
    ts = base.replace(second=tick % 60).isoformat(timespec="milliseconds").replace("+00:00", "Z")
    return ts


def build_udp_lines(src: str, tick: int, seq_start: int) -> tuple[list[str], int]:
    """Return deterministic lines for one source/tick and next sequence value."""
    seq = seq_start
    lines: list[str] = []


    lines.append(_msg(src, tick, seq, "sync", f"{src} synchronized step {tick:03d}"))
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


class InjectFanout:
    def __init__(self, targets: list[Target], connect_timeout: float):
        self._clients: dict[str, LogClient] = {}
        self._connect_timeout = connect_timeout
        for t in targets:
            client = LogClient(t.host, t.port, source="TEST", connect_timeout=connect_timeout)
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

    print("[det-demo] UDP targets:")
    for t in args.udp:
        print(f"  - {t.name} -> {t.host}:{t.port}")
    if args.inject:
        print("[det-demo] inject targets:")
        for t in args.inject:
            print(f"  - {t.name} -> {t.host}:{t.port}")
    print(f"[det-demo] tick_ms={args.tick_ms:g} cycles={'infinite' if args.cycles == 0 else args.cycles}")

    seq_by_src = {t.name: 1 for t in args.udp}
    inject = InjectFanout(args.inject, args.connect_timeout) if args.inject else None
    tick_interval = args.tick_ms / 1000.0
    per_source_offset = min(0.005, tick_interval / max(1, len(args.udp) * 4))

    tick = 0
    next_tick_at = time.monotonic()

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp_sock:
        try:
            while args.cycles == 0 or tick < args.cycles:
                tick += 1
                now = time.monotonic()
                if now < next_tick_at:
                    time.sleep(next_tick_at - now)

                for index, target in enumerate(args.udp):
                    if index > 0 and per_source_offset > 0:
                        time.sleep(per_source_offset)
                    lines, next_seq = build_udp_lines(target.name, tick, seq_by_src[target.name])
                    seq_by_src[target.name] = next_seq
                    payload = ("\n".join(lines) + "\n").encode("utf-8")
                    udp_sock.sendto(payload, (target.host, target.port))

                if inject is not None and tick % 10 == 0:
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
