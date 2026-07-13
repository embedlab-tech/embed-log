#!/usr/bin/env python3
"""Exercise the installed embed-log TUI through a real pseudo-terminal.

Starts two UDP sources in separate tabs, sends one record to each, cycles tabs,
and exits with ``q``. The TUI's state/key tests cover the exact tab/sync state
changes; this script verifies the installed CLI, server, WebSocket client, raw
terminal mode, and persisted source logs work together.
"""

from __future__ import annotations

import argparse
import os
import pty
import select
import signal
import socket
import tempfile
import time
from pathlib import Path


def free_port(sock_type: int) -> int:
    with socket.socket(socket.AF_INET, sock_type) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_for_marker(fd: int, output: bytearray, marker: bytes, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.1)
        if ready:
            try:
                output.extend(os.read(fd, 8192))
            except OSError:
                break
        if marker in output:
            return
    text = output.decode("utf-8", errors="replace")
    raise RuntimeError(f"did not observe {marker!r} before timeout:\n{text}")


def wait_for_exit(pid: int, fd: int, output: bytearray, timeout: float) -> int:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.1)
        if ready:
            try:
                output.extend(os.read(fd, 8192))
            except OSError:
                pass
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            return os.waitstatus_to_exitcode(status)
    os.kill(pid, signal.SIGKILL)
    _, status = os.waitpid(pid, 0)
    raise RuntimeError(f"TUI did not exit after q (killed with {os.waitstatus_to_exitcode(status)})")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path, help="installed embed-log executable")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit(f"not an executable embed-log binary: {binary}")

    ws_port = free_port(socket.SOCK_STREAM)
    alpha_port = free_port(socket.SOCK_DGRAM)
    beta_port = free_port(socket.SOCK_DGRAM)

    with tempfile.TemporaryDirectory(prefix="embed-log-tui-integration-") as directory:
        root = Path(directory)
        logs_dir = root / "logs"
        config = root / "tui.yml"
        config.write_text(
            f"""version: 1
server:
  host: 127.0.0.1
  ws_port: {ws_port}
  app_name: TUI integration test
logs:
  dir: {logs_dir}
sources:
  - name: ALPHA
    label: Alpha source
    type: udp
    port: {alpha_port}
  - name: BETA
    label: Beta source
    type: udp
    port: {beta_port}
tabs:
  - label: Alpha tab
    panes: [ALPHA]
  - label: Beta tab
    panes: [BETA]
""",
            encoding="utf-8",
        )

        pid, fd = pty.fork()
        if pid == 0:
            os.environ["TERM"] = "xterm-256color"
            os.execv(
                str(binary),
                [str(binary), "run", "--tui", "--config", str(config), "--no-open-browser"],
            )

        output = bytearray()
        try:
            wait_for_marker(fd, output, b"TUI WS connected", timeout=10)
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
                sender.sendto(b"alpha tab integration record", ("127.0.0.1", alpha_port))
                sender.sendto(b"beta tab integration record", ("127.0.0.1", beta_port))
            time.sleep(0.5)
            os.write(fd, b"\t")  # cycle Alpha tab -> Beta tab
            time.sleep(0.2)
            os.write(fd, b"q")
            exit_code = wait_for_exit(pid, fd, output, timeout=10)
        except Exception:
            try:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
            except ProcessLookupError:
                pass
            raise
        finally:
            os.close(fd)

        if exit_code != 0:
            raise RuntimeError(f"TUI exited with {exit_code}")
        source_logs = list(logs_dir.glob("*/*.log"))
        contents = "\n".join(path.read_text(encoding="utf-8", errors="replace") for path in source_logs)
        for record in ("alpha tab integration record", "beta tab integration record"):
            if record not in contents:
                raise RuntimeError(f"missing persisted TUI test record: {record}")

    print("TUI integration passed: connected, switched tabs, persisted both UDP sources, and quit cleanly")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
