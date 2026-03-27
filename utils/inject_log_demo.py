"""
Inject log demo — connects to one or more server inject ports and periodically
injects marker lines selected from a predefined message corpus.

CLI mirrors the server naming style by using repeated:
    --inject NAME PORT

Run the log server first:
    python3 backend/server.py \
        --source DEVICE_A uart:/dev/ttyUSB0 \
        --source DEVICE_B uart:/dev/ttyUSB1 \
        --inject DEVICE_A 5001 \
        --inject DEVICE_B 5002 \
        --tab "Devices" DEVICE_A DEVICE_B \
        --ws-port 8080

Then in a separate terminal:
    python3 utils/inject_log_demo.py \
        --inject DEVICE_A 5001 \
        --inject DEVICE_B 5002
"""

import argparse
import random
import sys
import threading
import time
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parents[1]
if str(ROOT_DIR) not in sys.path:
    sys.path.insert(0, str(ROOT_DIR))

from backend.log_client import LogClient

DEFAULT_INTERVAL = 10.0   # seconds between each cycle
DEFAULT_DURATION = 60.0   # total run time in seconds
DEFAULT_MESSAGES_FILE = Path(__file__).with_name("inject_messages.txt")

SEVERITY_COLORS = {
    "inf": "white",
    "dbg": "cyan",
    "wrn": "yellow",
    "err": "red",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate marker traffic against server inject ports."
    )
    parser.add_argument(
        "--inject",
        nargs=2,
        action="append",
        metavar=("NAME", "PORT"),
        required=True,
        help="Source name and inject TCP port (repeat for multiple sources).",
    )
    parser.add_argument(
        "--host",
        default="127.0.0.1",
        help="Inject host for all --inject entries (default: 127.0.0.1).",
    )
    parser.add_argument(
        "--interval",
        type=float,
        default=DEFAULT_INTERVAL,
        help=f"Seconds between cycles per source (default: {DEFAULT_INTERVAL}).",
    )
    parser.add_argument(
        "--duration",
        type=float,
        default=DEFAULT_DURATION,
        help=f"Total runtime in seconds, 0 means run forever (default: {DEFAULT_DURATION}).",
    )
    parser.add_argument(
        "--messages",
        type=Path,
        default=DEFAULT_MESSAGES_FILE,
        help=f"Path to message corpus file (default: {DEFAULT_MESSAGES_FILE.name}).",
    )
    parser.add_argument(
        "--source",
        default="demo",
        help="Marker source label visible in logs (default: demo).",
    )
    parser.add_argument("--seed", type=int, default=None, help="Optional random seed.")
    return parser.parse_args()


def _parse_inject_entries(entries: list[list[str]]) -> list[dict]:
    devices = []
    seen_names = set()
    seen_ports = set()
    for name, port_s in entries:
        if name in seen_names:
            raise ValueError(f"duplicate --inject name: {name!r}")
        try:
            port = int(port_s)
        except ValueError as exc:
            raise ValueError(f"--inject {name!r}: port must be integer, got {port_s!r}") from exc
        if not (1 <= port <= 65535):
            raise ValueError(f"--inject {name!r}: port out of range: {port}")
        if port in seen_ports:
            raise ValueError(f"duplicate --inject port: {port}")
        seen_names.add(name)
        seen_ports.add(port)
        devices.append({"name": name, "port": port})
    return devices


def _load_messages(path: Path) -> list[str]:
    try:
        raw = path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ValueError(f"messages file not found: {path}") from exc
    lines = [line.strip() for line in raw.splitlines() if line.strip()]
    if not lines:
        raise ValueError(f"messages file is empty: {path}")
    return lines


def _parse_message(raw: str) -> tuple[str, str]:
    if raw.startswith("<") and ">" in raw:
        tag, rest = raw[1:].split(">", 1)
        tag = tag.strip().lower()
        message = rest.strip()
        if message:
            return message, SEVERITY_COLORS.get(tag, "white")
    return raw, "white"


def device_writer(
    name: str,
    host: str,
    port: int,
    interval: float,
    marker_source: str,
    messages: list[str],
    seed: int,
    stop: threading.Event,
) -> None:
    counter = 0
    rng = random.Random(seed)
    with LogClient(host, port, source=marker_source, connect_timeout=30) as client:
        print(f"[inject-demo] connected to {name} on {host}:{port}")
        while not stop.wait(interval):
            counter += 1
            chosen = rng.choice(messages)
            text, color = _parse_message(chosen)
            client.marker(
                f"[{name}] {text} (cycle #{counter})",
                color=color,
            )


def main() -> None:
    args = parse_args()
    if args.interval <= 0:
        raise SystemExit("--interval must be > 0")
    if args.duration < 0:
        raise SystemExit("--duration must be >= 0")
    devices = _parse_inject_entries(args.inject)
    messages = _load_messages(args.messages)
    rng = random.Random(args.seed)

    stop = threading.Event()

    threads = [
        threading.Thread(
            target=device_writer,
            kwargs={
                "name": d["name"],
                "host": args.host,
                "port": d["port"],
                "interval": args.interval,
                "marker_source": args.source,
                "messages": messages,
                "seed": rng.randrange(0, 2**32),
                "stop": stop,
            },
            daemon=True,
            name=f"writer-{d['name']}",
        )
        for d in devices
    ]

    for t in threads:
        t.start()

    if args.duration == 0:
        print(
            f"[inject-demo] running until Ctrl+C — marker lines sent every "
            f"{args.interval:g}s to {len(devices)} source(s)"
        )
    else:
        print(
            f"[inject-demo] running for {args.duration:g}s — marker lines sent every "
            f"{args.interval:g}s to {len(devices)} source(s)"
        )
    try:
        if args.duration == 0:
            while True:
                time.sleep(3600)
        else:
            time.sleep(args.duration)
    except KeyboardInterrupt:
        pass
    finally:
        stop.set()
        print("[inject-demo] done")


if __name__ == "__main__":
    main()
