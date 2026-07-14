#!/usr/bin/env python3
"""Verify that embed-log tails an absolute-path file written by another process."""

from __future__ import annotations

import argparse
import json
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def unused_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def wait_for_port(port: int, process: subprocess.Popen[bytes], timeout: float = 10) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"embed-log exited before opening its server (exit {process.returncode})")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("embed-log server did not become ready")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path, help="embed-log executable to test")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        raise SystemExit(f"embed-log binary does not exist: {binary}")

    with tempfile.TemporaryDirectory(prefix="embed-log-file-watch-") as directory:
        root = Path(directory)
        watched = (root / "incoming.log").resolve()
        watched.write_text("pre-existing data is not replayed\n", encoding="utf-8")
        logs_dir = root / "sessions"
        port = unused_port()
        config = root / "watch.yml"
        config.write_text(
            f"""version: 1
server:
  host: 127.0.0.1
  ws_port: {port}
logs:
  dir: {json.dumps(str(logs_dir))}
sources:
  - name: ABSOLUTE_FILE
    type: file
    port: {json.dumps(str(watched))}
tabs:
  - label: File watch
    panes: [ABSOLUTE_FILE]
""",
            encoding="utf-8",
        )

        server = subprocess.Popen(
            [str(binary), "run", "--config", str(config), "--no-open-browser"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        producer: subprocess.Popen[bytes] | None = None
        try:
            wait_for_port(port, server)
            marker = "external-file-watcher-record"
            # Keep writing while the source task initializes. This models a real
            # producer and avoids depending on a scheduler-specific watcher-start
            # race: at least one post-watch append must be observed.
            producer = subprocess.Popen(
                [
                    sys.executable,
                    "-c",
                    (
                        "from pathlib import Path\n"
                        "import sys, time\n"
                        "path, marker = Path(sys.argv[1]), sys.argv[2]\n"
                        "for i in range(50):\n"
                        "    with path.open('a', encoding='utf-8') as output:\n"
                        "        output.write(f'{marker}-{i}\\n')\n"
                        "    time.sleep(.1)\n"
                    ),
                    str(watched),
                    marker,
                ]
            )

            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                if any(
                    marker in path.read_text(encoding="utf-8", errors="replace")
                    for path in logs_dir.rglob("*.log")
                ):
                    print("file-watch integration passed: absolute path received external appends")
                    return 0
                if server.poll() is not None:
                    raise RuntimeError(f"embed-log exited while watching (exit {server.returncode})")
                time.sleep(0.05)
            raise RuntimeError("embed-log did not persist an externally appended file record")
        finally:
            if producer and producer.poll() is None:
                producer.terminate()
                producer.wait(timeout=5)
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == "__main__":
    raise SystemExit(main())
