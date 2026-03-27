"""
Inject log demo — connects to one or more server inject ports and every interval:
  1. Writes a log marker (visible in log files and browser UI)
  2. Sends a TX command to the mapped source

CLI mirrors the server naming style by using repeated:
    --inject NAME PORT

Run the log server first:
    python3 backend/server.py \
        --source READER     uart:/dev/ttyFTDI_A \
        --source CONTROLLER uart:/dev/ttyFTDI_B \
        --inject READER     5001 \
        --inject CONTROLLER 5002 \
        --tab "Devices" READER CONTROLLER \
        --ws-port 8080

Then in a separate terminal:
    python3 utils/inject_log_demo.py \
        --inject READER 5001 \
        --inject CONTROLLER 5002
"""

import argparse
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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate marker + TX traffic against server inject ports."
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
        "--command",
        default="heap stat",
        help="TX command sent each cycle with sendline() (default: 'heap stat').",
    )
    parser.add_argument(
        "--source",
        default="demo",
        help="Marker source label visible in logs (default: demo).",
    )
    parser.add_argument(
        "--color",
        default="cyan",
        choices=["red", "green", "yellow", "blue", "magenta", "cyan", "white", "bold"],
        help="Marker color (default: cyan).",
    )
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


def device_writer(
    name: str,
    host: str,
    port: int,
    interval: float,
    marker_source: str,
    marker_color: str,
    command: str,
    stop: threading.Event,
) -> None:
    counter = 0
    with LogClient(host, port, source=marker_source, connect_timeout=30) as client:
        print(f"[inject-demo] connected to {name} on {host}:{port}")
        while not stop.wait(interval):
            counter += 1
            client.marker(
                f"[{name}] sending '{command}' (cycle #{counter})",
                color=marker_color,
            )
            client.sendline(command)


def main() -> None:
    args = parse_args()
    if args.interval <= 0:
        raise SystemExit("--interval must be > 0")
    if args.duration < 0:
        raise SystemExit("--duration must be >= 0")
    devices = _parse_inject_entries(args.inject)

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
                "marker_color": args.color,
                "command": args.command,
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
            f"[inject-demo] running until Ctrl+C — command {args.command!r} "
            f"sent every {args.interval:g}s to {len(devices)} source(s)"
        )
    else:
        print(
            f"[inject-demo] running for {args.duration:g}s — command {args.command!r} "
            f"sent every {args.interval:g}s to {len(devices)} source(s)"
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
