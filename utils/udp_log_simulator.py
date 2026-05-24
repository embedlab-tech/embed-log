#!/usr/bin/env python3
"""
udp_log_simulator.py

Generate synthetic UDP log traffic for embed-log UDP sources.
Each sent line is prepended with the local system timestamp.
"""

from __future__ import annotations

import argparse
import random
import socket
import sys
import time
from datetime import datetime
from pathlib import Path


DEFAULT_MESSAGES_FILE = Path(__file__).with_name("sim_messages.txt")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Send randomized timestamped log lines to one or more UDP targets. "
            "Useful for testing embed-log without serial hardware."
        )
    )
    parser.add_argument(
        "--target",
        action="append",
        default=[],
        metavar="HOST:PORT",
        help=(
            "UDP destination. Repeat to send to multiple sockets "
            "(example: --target 127.0.0.1:6000 --target 127.0.0.1:6001)"
        ),
    )
    parser.add_argument(
        "--port",
        type=int,
        action="append",
        default=[],
        metavar="PORT",
        help=(
            "UDP destination port using --host (repeat allowed). "
            "Equivalent to --target <host>:PORT."
        ),
    )
    parser.add_argument(
        "--host",
        default="127.0.0.1",
        help="Host used with --port (default: 127.0.0.1).",
    )
    parser.add_argument(
        "--messages",
        type=Path,
        default=DEFAULT_MESSAGES_FILE,
        help=f"Path to message corpus file (default: {DEFAULT_MESSAGES_FILE.name}).",
    )
    parser.add_argument(
        "--interval-min",
        type=float,
        default=0.05,
        metavar="SECONDS",
        help="Minimum random delay between sends (default: 0.05).",
    )
    parser.add_argument(
        "--interval-max",
        type=float,
        default=0.40,
        metavar="SECONDS",
        help="Maximum random delay between sends (default: 0.40).",
    )
    parser.add_argument(
        "--count",
        type=int,
        default=0,
        metavar="N",
        help="Total number of lines to send, 0 means infinite (default: 0).",
    )
    parser.add_argument(
        "--burst-min",
        type=int,
        default=1,
        metavar="N",
        help="Minimum lines per send burst (default: 1).",
    )
    parser.add_argument(
        "--burst-max",
        type=int,
        default=3,
        metavar="N",
        help="Maximum lines per send burst (default: 3).",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=None,
        help="Optional random seed for reproducible output.",
    )
    return parser.parse_args()


def parse_target(value: str) -> tuple[str, int]:
    if ":" not in value:
        raise ValueError(f"invalid --target {value!r}, expected HOST:PORT")
    host, port_str = value.rsplit(":", 1)
    if not host:
        raise ValueError(f"invalid --target {value!r}, host is empty")
    try:
        port = int(port_str)
    except ValueError as exc:
        raise ValueError(f"invalid --target {value!r}, port must be integer") from exc
    validate_port(port)
    return host, port


def validate_port(port: int) -> None:
    if not (1 <= port <= 65535):
        raise ValueError(f"invalid UDP port {port}, expected 1..65535")


def load_messages(path: Path) -> list[str]:
    try:
        raw = path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ValueError(f"messages file not found: {path}") from exc

    messages = [line.strip() for line in raw.splitlines() if line.strip()]
    if not messages:
        raise ValueError(f"messages file is empty: {path}")
    return messages


def resolve_targets(args: argparse.Namespace) -> list[tuple[str, int]]:
    targets: list[tuple[str, int]] = []
    for target in args.target:
        targets.append(parse_target(target))

    for port in args.port:
        validate_port(port)
        targets.append((args.host, port))

    if not targets:
        raise ValueError("no UDP targets provided, use --target and/or --port")

    deduped: list[tuple[str, int]] = []
    seen: set[tuple[str, int]] = set()
    for target in targets:
        if target not in seen:
            deduped.append(target)
            seen.add(target)
    return deduped


def now_timestamp() -> str:
    return datetime.now().astimezone().isoformat(timespec="milliseconds")


def build_log_line(message: str) -> str:
    return message


def run(args: argparse.Namespace) -> int:
    if args.interval_min <= 0 or args.interval_max <= 0:
        raise ValueError("interval values must be > 0")
    if args.interval_min > args.interval_max:
        raise ValueError("--interval-min must be <= --interval-max")
    if args.count < 0:
        raise ValueError("--count must be >= 0")
    if args.burst_min <= 0 or args.burst_max <= 0:
        raise ValueError("burst values must be > 0")
    if args.burst_min > args.burst_max:
        raise ValueError("--burst-min must be <= --burst-max")

    targets = resolve_targets(args)
    messages = load_messages(args.messages)
    rng = random.Random(args.seed)

    print(f"Loaded {len(messages)} messages from {args.messages}")
    print("Sending UDP logs to:")
    for host, port in targets:
        print(f"  - {host}:{port}")
    if args.seed is not None:
        print(f"Using random seed: {args.seed}")
    if args.count == 0:
        print("Mode: infinite (press Ctrl+C to stop)")
    else:
        print(f"Mode: finite ({args.count} lines)")

    sent = 0
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        try:
            while args.count == 0 or sent < args.count:
                burst_size = rng.randint(args.burst_min, args.burst_max)
                remaining = args.count - sent if args.count > 0 else burst_size
                this_burst = min(burst_size, remaining) if args.count > 0 else burst_size

                target = rng.choice(targets)
                lines = [build_log_line(rng.choice(messages)) for _ in range(this_burst)]
                payload = ("\n".join(lines) + "\n").encode("utf-8")
                sock.sendto(payload, target)

                sent += this_burst
                print(f"sent {this_burst:>2} line(s) to {target[0]}:{target[1]}  total={sent}")
                time.sleep(rng.uniform(args.interval_min, args.interval_max))
        except KeyboardInterrupt:
            print("\nInterrupted by user.")

    print(f"Done. Sent {sent} line(s).")
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
