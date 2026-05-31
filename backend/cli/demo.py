"""Embed-log demo subcommand — port of run_demo.sh."""

from __future__ import annotations

import argparse
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


# Ports used by the demo setup
INJECT_PORTS = [5001, 5002, 5003]
UDP_PORTS = [6000, 6001, 6002, 6003]
WS_PORT = 8080
ALL_PORTS = INJECT_PORTS + UDP_PORTS + [WS_PORT]

ROOT = Path(__file__).resolve().parents[2]
DEMO_CONFIG = ROOT / "embed-log.demo.yml"
DEMO_UTILS = ROOT / "utils"


def _port_in_use(port: int, proto: str = "tcp") -> bool:
    """Check if a local port is in use."""
    if proto == "tcp":
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            return s.connect_ex(("127.0.0.1", port)) == 0
    else:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            try:
                s.bind(("127.0.0.1", port))
                return False
            except OSError:
                return True


def _try_kill_port_pid(port: int, proto: str = "tcp") -> bool:
    """Try to find and kill the process holding a port (macOS/Linux)."""
    import subprocess as _sp
    try:
        if proto == "tcp":
            out = _sp.run(
                ["lsof", "-tiTCP", str(port), "-sTCP:LISTEN"],
                capture_output=True, text=True, timeout=5,
            )
        else:
            out = _sp.run(
                ["lsof", "-tiUDP", str(port)],
                capture_output=True, text=True, timeout=5,
            )
    except (FileNotFoundError, _sp.TimeoutExpired):
        return False
    pids = [int(p) for p in out.stdout.strip().split() if p.strip().isdigit()]
    if not pids:
        return False
    for pid in pids:
        try:
            os.kill(pid, signal.SIGTERM)
            for _ in range(10):
                time.sleep(0.15)
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
            else:
                os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    return True


def _free_port(port: int, proto: str = "tcp") -> None:
    if not _port_in_use(port, proto):
        return
    _try_kill_port_pid(port, proto)
    time.sleep(0.5)
    if _port_in_use(port, proto):
        print(f"ERROR: {proto} port {port} is in use by a non-demo process.", file=sys.stderr)
        sys.exit(1)


class DemoRunner:
    """Manages a local embed-log demo session with traffic simulation."""

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self._processes: list[subprocess.Popen] = []
        self._server_pid: int | None = None

    def run(self) -> int:
        """Start the demo and wait for it to finish."""
        if not DEMO_CONFIG.is_file():
            print(f"Demo config not found: {DEMO_CONFIG}", file=sys.stderr)
            print("Run from the repository root or ensure embed-log.demo.yml exists.", file=sys.stderr)
            return 1

        # ── Port cleanup ──
        print("Checking demo ports...")
        for p in ALL_PORTS:
            proto = "udp" if p in UDP_PORTS else "tcp"
            _free_port(p, proto)

        # ── Start server ──
        python = sys.executable
        server_cmd = [
            python, str(ROOT / "backend" / "server.py"),
            "run", "--config", str(DEMO_CONFIG),
            "--ws-port", str(WS_PORT),
        ]
        if self.args.no_browser:
            server_cmd.append("--no-open-browser")
        if self.args.log_dir:
            server_cmd.extend(["--log-dir", self.args.log_dir])
        if self.args.verbose:
            server_cmd.append("-v")

        print(f"Starting embed-log server on port {WS_PORT}...")
        server = subprocess.Popen(server_cmd)
        self._server_pid = server.pid
        self._processes.append(server)

        time.sleep(1)
        if server.poll() is not None:
            print("ERROR: embed-log server failed to start.", file=sys.stderr)
            self._cleanup()
            return 1
        # ── Resolve defaults with --fast ──
        if self.args.fast:
            if self.args.interval_min is None:
                self.args.interval_min = 0.10
            if self.args.interval_max is None:
                self.args.interval_max = 0.30
            if self.args.inject_interval is None:
                self.args.inject_interval = 1.0
            if self.args.tick_ms is None:
                self.args.tick_ms = 20
        else:
            if self.args.interval_min is None:
                self.args.interval_min = 5.0
            if self.args.interval_max is None:
                self.args.interval_max = 20.0
            if self.args.inject_interval is None:
                self.args.inject_interval = 5.0
            if self.args.tick_ms is None:
                self.args.tick_ms = 100

        # ── Start traffic ──
        profile = self.args.profile
        if profile == "random":
            self._start_random_traffic()
        elif profile == "test" or profile == "deterministic":
            self._start_deterministic_traffic()
        else:
            print(f"Unknown profile: {profile}", file=sys.stderr)
            self._cleanup()
            return 1

        print("")
        print("Demo running!")
        print(f"Open: http://127.0.0.1:{WS_PORT}/")
        print("Press Ctrl+C to stop all processes.")
        print("")

        # ── Wait for any process to exit ──
        try:
            while True:
                for p in self._processes[:]:
                    ret = p.poll()
                    if ret is not None and ret != -9:
                        self._processes.remove(p)
                if not self._processes:
                    break
                time.sleep(0.5)
        except KeyboardInterrupt:
            pass
        finally:
            self._cleanup()
        return 0

    def _start_random_traffic(self) -> None:
        """Start the random UDP simulator + injector + CBOR traffic."""
        sim_script = DEMO_UTILS / "udp_log_simulator.py"
        if not sim_script.is_file():
            print(f"Traffic script not found: {sim_script}", file=sys.stderr)
            return

        cmd = [
            sys.executable, str(sim_script),
            "--target", "127.0.0.1:6000",
            "--target", "127.0.0.1:6001",
            "--target", "127.0.0.1:6002",
            "--interval-min", str(self.args.interval_min),
            "--interval-max", str(self.args.interval_max),
        ]
        print(f"Starting UDP simulator (interval {self.args.interval_min}-{self.args.interval_max}s)...")
        self._processes.append(subprocess.Popen(cmd))

        inject_script = DEMO_UTILS / "inject_log_demo.py"
        if inject_script.is_file():
            inject_cmd = [
                sys.executable, str(inject_script),
                "--inject", "SENSOR_A", "5001",
                "--inject", "SENSOR_B", "5002",
                "--inject", "SENSOR_C", "5003",
                "--interval", str(self.args.inject_interval),
                "--duration", "0",
                "--source", "demo",
            ]
            print(f"Starting marker injector (interval {self.args.inject_interval}s)...")
            self._processes.append(subprocess.Popen(inject_cmd))

        deterministic_script = DEMO_UTILS / "deterministic_demo_traffic.py"
        if deterministic_script.is_file():
            cbor_cmd = [
                sys.executable, str(deterministic_script),
                "--cbor",
                "--udp", "SENSOR_CBOR=127.0.0.1:6003",
                "--tick-ms", "500",
                "--cycles", "0",
            ]
            print("Starting CBOR demo traffic (tick 500ms)...")
            self._processes.append(subprocess.Popen(cbor_cmd))

    def _start_deterministic_traffic(self) -> None:
        """Start deterministic demo traffic for UI tests."""
        deterministic_script = DEMO_UTILS / "deterministic_demo_traffic.py"
        if not deterministic_script.is_file():
            print(f"Traffic script not found: {deterministic_script}", file=sys.stderr)
            return

        cmd = [
            sys.executable, str(deterministic_script),
            "--udp", "SENSOR_A=127.0.0.1:6000",
            "--udp", "SENSOR_B=127.0.0.1:6001",
            "--udp", "SENSOR_C=127.0.0.1:6002",
            "--tick-ms", str(self.args.tick_ms),
            "--cycles", "0",
        ]
        print(f"Starting deterministic demo traffic (tick {self.args.tick_ms}ms)...")
        self._processes.append(subprocess.Popen(cmd))

        cbor_cmd = [
            sys.executable, str(deterministic_script),
            "--cbor",
            "--udp", "SENSOR_CBOR=127.0.0.1:6003",
            "--tick-ms", str(self.args.tick_ms),
            "--cycles", "0",
        ]
        print(f"Starting CBOR deterministic demo traffic (tick {self.args.tick_ms}ms)...")
        self._processes.append(subprocess.Popen(cbor_cmd))

    def _cleanup(self) -> None:
        """Stop all child processes."""
        print("")
        print("Stopping demo...")

        # SIGTERM children (reverse order: traffic first, server last)
        for p in reversed(self._processes):
            try:
                p.terminate()
            except ProcessLookupError:
                pass

        # Give the server extra time for session export
        if self._server_pid is not None:
            try:
                os.kill(self._server_pid, 0)
                for _ in range(10):
                    time.sleep(0.3)
                    try:
                        os.kill(self._server_pid, 0)
                    except ProcessLookupError:
                        break
            except ProcessLookupError:
                pass

        time.sleep(0.4)

        # SIGKILL remaining
        for p in self._processes:
            try:
                p.kill()
                p.wait(timeout=2)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                pass
        self._processes.clear()


def _run_demo(args: argparse.Namespace) -> int:
    runner = DemoRunner(args)
    return runner.run()


def add_subparser(subparsers) -> None:
    p = subparsers.add_parser(
        "demo",
        help="start a local demo server with simulated traffic",
        description="Start embed-log with a demo config and simulated log traffic.",
        epilog=(
            "Examples:\n"
            "  embed-log demo\n"
            "  embed-log demo --profile test\n"
            "  embed-log demo --profile random --no-browser\n"
            "  embed-log demo --profile deterministic --fast\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--profile", choices=["random", "test", "deterministic"],
        default="random",
        help="traffic profile (default: random)",
    )
    p.add_argument(
        "--no-browser", action="store_true",
        help="do not open browser automatically",
    )
    p.add_argument(
        "--verbose", "-v", action="store_true",
        help="enable server event logging",
    )
    p.add_argument(
        "--log-dir",
        help="override the log output directory",
    )
    p.add_argument(
        "--fast", action="store_true",
        help="use faster intervals for testing",
    )
    p.add_argument(
        "--tick-ms", type=int, default=None,
        help="deterministic profile tick interval in ms (default: 100, fast: 20)",
    )
    p.add_argument(
        "--interval-min", type=float, default=None,
        help="random profile minimum interval in seconds (default: 5.0, fast: 0.1)",
    )
    p.add_argument(
        "--interval-max", type=float, default=None,
        help="random profile maximum interval in seconds (default: 20.0, fast: 0.3)",
    )
    p.add_argument(
        "--inject-interval", type=float, default=None,
        help="random profile inject interval in seconds (default: 5, fast: 1)",
    )
