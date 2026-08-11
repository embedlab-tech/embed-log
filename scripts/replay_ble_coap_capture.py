#!/usr/bin/env python3
"""Replay pipe-delimited BLE/CoAP captures to an embed-log UDP hex-CoAP source.

Input rows have this form (the header is optional):
  timestamp | transport | direction | source_port | destination_port | payload

The original row, including its capture metadata, is sent as one newline-delimited
UDP log line. This is intentional: embed-log's `hex-coap` parser preserves the
prefix and replaces the first valid hexadecimal CoAP packet with its decode.
"""

from __future__ import annotations

import argparse
import re
import socket
import sys
import time
from pathlib import Path

HEX = re.compile(r"^[0-9A-Fa-f]+$")


def capture_rows(path: Path) -> list[str]:
    rows: list[str] = []
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#") or line.lower().startswith("timestamp|"):
            continue
        columns = [column.strip() for column in line.split("|", maxsplit=5)]
        if len(columns) != 6:
            raise ValueError(f"{path}:{line_number}: expected 6 pipe-delimited columns")
        payload = columns[-1].replace(" ", "")
        if len(payload) % 2 or not HEX.fullmatch(payload):
            raise ValueError(f"{path}:{line_number}: payload is not even-length hexadecimal")
        rows.append(line)
    if not rows:
        raise ValueError(f"{path}: no capture rows found")
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="pipe-delimited capture text file")
    parser.add_argument("--host", default="127.0.0.1", help="UDP destination host (default: 127.0.0.1)")
    parser.add_argument("--port", type=int, default=10001, help="UDP destination port (default: 10001)")
    parser.add_argument("--interval", type=float, default=0.1, help="seconds between rows (default: 0.1)")
    parser.add_argument("--repeat", action="store_true", help="repeat the capture until interrupted")
    args = parser.parse_args()
    if args.interval < 0:
        parser.error("--interval must be non-negative")

    rows = capture_rows(args.input)
    print(f"Replaying {len(rows)} capture rows to udp://{args.host}:{args.port} every {args.interval:.3f}s")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
        sent = 0
        while True:
            for row in rows:
                udp.sendto((row + "\n").encode("utf-8"), (args.host, args.port))
                sent += 1
                print(f"[{sent}] {row}")
                if args.interval:
                    time.sleep(args.interval)
            if not args.repeat:
                break
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
