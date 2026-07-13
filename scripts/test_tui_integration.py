#!/usr/bin/env python3
"""Exercise the installed embed-log TUI through real pseudo-terminals.

``simulated`` uses two PTY UARTs and is suitable for ordinary CI. ``stm32g0``
uses the connected ST-LINK control shell plus the three FTDI generator UARTs.
Both backends start the installed CLI's ``run --tui`` mode, cycle tabs, and
verify that the expected records were persisted by the TUI-hosted server.
"""

from __future__ import annotations

import argparse
import os
import pty
import re
import select
import shutil
import signal
import socket
import tempfile
import time
from pathlib import Path

DEFAULT_PORTS = {
    "CONTROL": "/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_0669FF485552787187184556-if02",
    "USART1": "/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if03-port0",
    "USART3": "/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if02-port0",
    "USART4": "/dev/serial/by-id/usb-FTDI_Quad_RS232-HS-if00-port0",
}
UART_PROFILES = {
    "USART1": {"peripheral": "uart1", "baudrate": 115200, "interval_ms": 10},
    "USART3": {"peripheral": "uart3", "baudrate": 460800, "interval_ms": 10},
    "USART4": {"peripheral": "uart4", "baudrate": 1000000, "interval_ms": 10},
}
HARDWARE_LINE_COUNT = 100


def free_port(sock_type: int) -> int:
    with socket.socket(socket.AF_INET, sock_type) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def drain(fd: int, output: bytearray, seconds: float) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.1)
        if ready:
            try:
                output.extend(os.read(fd, 8192))
            except OSError:
                return


def wait_for_marker(fd: int, output: bytearray, marker: bytes, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        drain(fd, output, 0.1)
        if marker in output:
            return
    raise RuntimeError(f"did not observe {marker!r} before timeout")


def wait_for_exit(pid: int, fd: int, output: bytearray, timeout: float) -> int:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        drain(fd, output, 0.1)
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            return os.waitstatus_to_exitcode(status)
    os.kill(pid, signal.SIGKILL)
    _, status = os.waitpid(pid, 0)
    raise RuntimeError(f"TUI did not exit after q (killed with {os.waitstatus_to_exitcode(status)})")


def start_tui(binary: Path, config: Path) -> tuple[int, int, bytearray]:
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.execv(str(binary), [str(binary), "run", "--tui", "--config", str(config), "--no-open-browser"])
    output = bytearray()
    try:
        wait_for_marker(fd, output, b"TUI WS connected", timeout=10)
    except Exception:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
        os.close(fd)
        raise
    return pid, fd, output


def stop_tui(pid: int, fd: int, output: bytearray) -> None:
    try:
        os.write(fd, b"q")
        exit_code = wait_for_exit(pid, fd, output, timeout=10)
        if exit_code != 0:
            raise RuntimeError(f"TUI exited with {exit_code}")
    finally:
        os.close(fd)


def write_config(path: Path, logs_dir: Path, ws_port: int, sources: list[dict], tabs: list[tuple[str, list[str]]]) -> None:
    lines = [
        "version: 1",
        "server:",
        "  host: 127.0.0.1",
        f"  ws_port: {ws_port}",
        "  app_name: TUI integration test",
        "logs:",
        f"  dir: {logs_dir}",
        "sources:",
    ]
    for source in sources:
        lines.extend(
            [
                f"  - name: {source['name']}",
                f"    label: {source['label']}",
                "    type: uart",
                f"    port: {source['port']}",
                f"    baudrate: {source['baudrate']}",
            ]
        )
    lines.append("tabs:")
    for label, panes in tabs:
        lines.extend([f"  - label: {label}", f"    panes: [{', '.join(panes)}]"])
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def source_messages(logs_dir: Path, source: str) -> list[str]:
    source_log = next(logs_dir.glob(f"*/*__{source.lower()}__*.log"), None)
    if source_log is None:
        raise RuntimeError(f"missing {source} source log under {logs_dir}")
    timestamp = re.compile(r"^\[[^\]]+\]\s?(.*)$")
    return [timestamp.sub(r"\1", line) for line in source_log.read_text(errors="replace").splitlines()]


def run_simulated(binary: Path, root: Path) -> None:
    masters: list[int] = []
    try:
        sources = []
        for name, label in (("ALPHA", "Alpha source"), ("BETA", "Beta source")):
            master, slave = pty.openpty()
            masters.append(master)
            port = os.ttyname(slave)
            os.close(slave)
            sources.append({"name": name, "label": label, "port": port, "baudrate": 115200})
        logs_dir = root / "logs"
        config = root / "tui.yml"
        write_config(
            config,
            logs_dir,
            free_port(socket.SOCK_STREAM),
            sources,
            [("Alpha tab", ["ALPHA"]), ("Beta tab", ["BETA"])],
        )
        pid, fd, output = start_tui(binary, config)
        try:
            # The UI connection is ready before the background UART readers may
            # have opened both PTY slaves.
            drain(fd, output, 0.5)
            # Opening both PTY readers is asynchronous. Repeat the small
            # idempotent records so a slow reader cannot make CI flaky.
            for _ in range(3):
                os.write(masters[0], b"alpha tab integration record\r\n")
                os.write(masters[1], b"beta tab integration record\r\n")
                drain(fd, output, 0.2)
            os.write(fd, b"\t")  # Alpha tab -> Beta tab
            drain(fd, output, 0.2)
        finally:
            stop_tui(pid, fd, output)
        for source, record in (("ALPHA", "alpha tab integration record"), ("BETA", "beta tab integration record")):
            if record not in source_messages(logs_dir, source):
                raise RuntimeError(f"missing persisted simulated UART record: {record}")
    finally:
        for master in masters:
            os.close(master)


def send_shell_command(fd: int, output: bytearray, command: str) -> None:
    # ':' opens TUI TX mode on the active CONTROL UART; Enter sends CR-normalized
    # data through the server's UART TX route to the Zephyr shell.
    os.write(fd, b":" + command.encode() + b"\r")
    drain(fd, output, 0.35)


def longest_contiguous_block(counters: list[int]) -> list[int]:
    """Return the longest ordered counter run from a source session log."""
    best: list[int] = []
    current: list[int] = []
    for counter in counters:
        if not current or counter == current[-1] + 1:
            current.append(counter)
        else:
            if len(current) > len(best):
                best = current
            current = [counter]
    return current if len(current) > len(best) else best


def run_stm32g0(binary: Path, root: Path, ports: dict[str, str]) -> None:
    missing = [f"{name}={path}" for name, path in ports.items() if not Path(path).exists()]
    if missing:
        raise RuntimeError("STM32G0 UART paths are unavailable: " + ", ".join(missing))
    logs_dir = root / "logs"
    config = root / "tui-stm32g0.yml"
    sources = [
        {"name": "CONTROL", "label": "Control shell", "port": ports["CONTROL"], "baudrate": 115200},
        *[
            {"name": name, "label": name, "port": ports[name], "baudrate": profile["baudrate"]}
            for name, profile in UART_PROFILES.items()
        ],
    ]
    write_config(
        config,
        logs_dir,
        free_port(socket.SOCK_STREAM),
        sources,
        [("Control", ["CONTROL"]), ("Generators A", ["USART1", "USART3"]), ("Generators B", ["USART4"])],
    )
    pid, fd, output = start_tui(binary, config)
    try:
        send_shell_command(fd, output, "scenario stop")
        for profile in UART_PROFILES.values():
            send_shell_command(fd, output, f"uart {profile['peripheral']} baud {profile['baudrate']}")
        for profile in UART_PROFILES.values():
            send_shell_command(fd, output, f"gen {profile['peripheral']} interval {profile['interval_ms']}")
            send_shell_command(fd, output, f"gen {profile['peripheral']} random off")
            send_shell_command(fd, output, f"gen {profile['peripheral']} start")
        drain(fd, output, 4.0)
        os.write(fd, b"\t")  # Control tab -> generator tab
        drain(fd, output, 0.2)
    finally:
        try:
            send_shell_command(fd, output, "scenario stop")
            for profile in UART_PROFILES.values():
                if profile["baudrate"] != 115200:
                    send_shell_command(fd, output, f"uart {profile['peripheral']} baud 115200")
        finally:
            stop_tui(pid, fd, output)

    for source in UART_PROFILES:
        pattern = re.compile(rf"^\[{source}\] INFO Counter=(\d+)$")
        counters = [int(match.group(1)) for message in source_messages(logs_dir, source) if (match := pattern.fullmatch(message))]
        block = longest_contiguous_block(counters)
        if len(block) < HARDWARE_LINE_COUNT:
            raise RuntimeError(
                f"{source} persisted no contiguous {HARDWARE_LINE_COUNT}-record hardware block"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path, help="installed embed-log executable")
    parser.add_argument("--backend", choices=("simulated", "stm32g0"), default="simulated")
    parser.add_argument("--artifact-dir", type=Path, help="keep test logs here instead of a temporary directory")
    parser.add_argument("--control-port", default=DEFAULT_PORTS["CONTROL"])
    parser.add_argument("--usart1-port", default=DEFAULT_PORTS["USART1"])
    parser.add_argument("--usart3-port", default=DEFAULT_PORTS["USART3"])
    parser.add_argument("--usart4-port", default=DEFAULT_PORTS["USART4"])
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit(f"not an executable embed-log binary: {binary}")

    if args.artifact_dir:
        root = args.artifact_dir.resolve()
        shutil.rmtree(root, ignore_errors=True)
        root.mkdir(parents=True)
        context = None
    else:
        context = tempfile.TemporaryDirectory(prefix=f"embed-log-tui-{args.backend}-")
        root = Path(context.name)
    try:
        if args.backend == "simulated":
            run_simulated(binary, root)
        else:
            run_stm32g0(
                binary,
                root,
                {"CONTROL": args.control_port, "USART1": args.usart1_port, "USART3": args.usart3_port, "USART4": args.usart4_port},
            )
    finally:
        if context is not None:
            context.cleanup()

    print(f"TUI {args.backend} integration passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
