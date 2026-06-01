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
INJECT_PORTS = [5001, 5002, 5003, 5004, 5005]
UDP_PORTS = [6000, 6001, 6002, 6003, 6004, 6005]
WS_PORT = 8080
ALL_PORTS = INJECT_PORTS + UDP_PORTS + [WS_PORT]

ROOT = Path(__file__).resolve().parents[2]
DEMO_UTILS = ROOT / "utils"


def _resolve_demo_config() -> Path | None:
    from . import _resolve_bundled_file

    return _resolve_bundled_file(
        "embed-log.demo.yml",
        packaged_relative="embed-log.demo.yml",
    )


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
                ["lsof", f"-tiTCP:{port}", "-sTCP:LISTEN"],
                capture_output=True, text=True, timeout=5,
            )
        else:
            out = _sp.run(
                ["lsof", f"-tiUDP:{port}"],
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
    for attempt in range(3):
        _try_kill_port_pid(port, proto)
        time.sleep(0.5)
        if not _port_in_use(port, proto):
            return
    print(f"ERROR: {proto} port {port} is in use by a non-demo process.", file=sys.stderr)
    sys.exit(1)


class DemoRunner:
    """Manages a local embed-log demo session with traffic simulation."""

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        # (subprocess.Popen, restart_command_or_None)
        # restart_command is None for the server process (never auto-restart)
        self._processes: list[tuple[subprocess.Popen, list[str] | None]] = []
        self._server_pid: int | None = None

    def _content_cycles(self, content_mode: str) -> int:
        """Return number of traffic cycles: 0 = infinite (with --continuous or
        --cycles 0); finite otherwise so the demo runs once and stops."""
        if self.args.continuous:
            return 0
        if self.args.cycles is not None:
            return self.args.cycles
        return 20 if content_mode == "curated" else 100

    def run(self) -> int:
        """Start the demo and wait for it to finish."""
        demo_config = _resolve_demo_config()
        if demo_config is None:
            print("Demo config not found.", file=sys.stderr)
            print("Ensure embed-log.demo.yml is bundled or run from the repository root.", file=sys.stderr)
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
            "run", "--config", str(demo_config),
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
        self._processes.append((server, None))

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
                self.args.tick_ms = 50
        else:
            if self.args.interval_min is None:
                self.args.interval_min = 5.0
            if self.args.interval_max is None:
                self.args.interval_max = 20.0
            if self.args.inject_interval is None:
                self.args.inject_interval = 5.0
            if self.args.tick_ms is None:
                self.args.tick_ms = 300

        # ── Start traffic ──
        profile = self.args.profile
        if profile == "random":
            self._start_random_traffic()
        elif profile == "test" or profile == "deterministic":
            self._start_deterministic_traffic(content_mode="test")
        elif profile == "curated":
            self._start_deterministic_traffic(content_mode="curated")
        else:
            print(f"Unknown profile: {profile}", file=sys.stderr)
            self._cleanup()
            return 1

        print("")
        print("Demo running!")
        print(f"Open: http://127.0.0.1:{WS_PORT}/")
        print("Press Ctrl+C to stop all processes.")
        print("")

        # ── Wait for any process to exit; restart traffic when --continuous ──
        try:
            while True:
                for item in list(self._processes):
                    proc, cmd = item
                    ret = proc.poll()
                    if ret is not None and ret != -9:
                        if cmd is not None and self.args.continuous:
                            # Traffic process exited — restart it
                            print(f"Traffic process (pid={proc.pid}) exited with code {ret}, restarting...")
                            try:
                                new_proc = subprocess.Popen(cmd)
                                self._processes[self._processes.index(item)] = (new_proc, cmd)
                            except OSError as e:
                                print(f"Failed to restart traffic: {e}", file=sys.stderr)
                                self._processes.remove(item)
                        else:
                            self._processes.remove(item)
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
            "--target", "127.0.0.1:6004",
            "--interval-min", str(self.args.interval_min),
            "--interval-max", str(self.args.interval_max),
        ]
        print(f"Starting UDP simulator (interval {self.args.interval_min}-{self.args.interval_max}s)...")
        self._processes.append((subprocess.Popen(cmd), cmd))

        inject_script = DEMO_UTILS / "inject_log_demo.py"
        if inject_script.is_file():
            inject_cmd = [
                sys.executable, str(inject_script),
                "--inject", "SENSOR_A", "5001",
                "--inject", "SENSOR_B", "5002",
                "--inject", "SENSOR_C", "5003",
                "--inject", "SENSOR_D", "5004",
                "--interval", str(self.args.inject_interval),
                "--duration", "0",
                "--source", "demo",
            ]
            print(f"Starting marker injector (interval {self.args.inject_interval}s)...")
            self._processes.append((subprocess.Popen(inject_cmd), inject_cmd))

        deterministic_script = DEMO_UTILS / "deterministic_demo_traffic.py"
        if deterministic_script.is_file():
            cbor_cmd = [
                sys.executable, str(deterministic_script),
                "--cbor",
                "--udp", "SENSOR_CBOR=127.0.0.1:6003",
                "--tick-ms", "500",
                "--cycles", str(self._content_cycles("curated")),
            ]
            print("Starting CBOR demo traffic (tick 500ms)...")
            self._processes.append((subprocess.Popen(cbor_cmd), cbor_cmd))

    def _start_deterministic_traffic(self, content_mode: str = "test") -> None:
        """Start deterministic demo traffic (test or curated content)."""
        deterministic_script = DEMO_UTILS / "deterministic_demo_traffic.py"
        if not deterministic_script.is_file():
            print(f"Traffic script not found: {deterministic_script}", file=sys.stderr)
            return

        cmd = [
            sys.executable, str(deterministic_script),
            "--content", content_mode,
            "--udp", "SENSOR_A=127.0.0.1:6000",
            "--udp", "SENSOR_B=127.0.0.1:6001",
            "--udp", "SENSOR_C=127.0.0.1:6002",
            "--udp", "SENSOR_D=127.0.0.1:6004",
            "--udp", "SENSOR_COAP=127.0.0.1:6005",
            "--tick-ms", str(self.args.tick_ms),
            "--cycles", str(self._content_cycles(content_mode)),
        ]
        label = "curated demo" if content_mode == "curated" else "deterministic test"
        print(f"Starting {label} traffic (tick {self.args.tick_ms}ms)...")
        self._processes.append((subprocess.Popen(cmd), cmd))

        cbor_cmd = [
            sys.executable, str(deterministic_script),
            "--content", content_mode,
            "--cbor",
            "--udp", "SENSOR_CBOR=127.0.0.1:6003",
            "--tick-ms", str(self.args.tick_ms),
            "--cycles", str(self._content_cycles(content_mode)),
        ]
        print(f"Starting CBOR {label} traffic (tick {self.args.tick_ms}ms)...")
        self._processes.append((subprocess.Popen(cbor_cmd), cbor_cmd))

    def _cleanup(self) -> None:
        """Stop all child processes."""
        print("")
        print("Stopping demo...")

        # SIGTERM children (reverse order: traffic first, server last)
        for item in reversed(self._processes):
            proc, _ = item
            try:
                proc.terminate()
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
        for item in self._processes:
            proc, _ = item
            try:
                proc.kill()
                proc.wait(timeout=2)
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
            "  embed-log demo --fast\n"
            "  embed-log demo --profile deterministic --fast\n"
            "  embed-log demo --profile random --no-browser\n"
            "  embed-log demo --profile deterministic --fast --continuous\n"
            "  embed-log demo --cycles 50\n"
            "  embed-log demo --cycles 0\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--profile", choices=["random", "test", "deterministic", "curated"],
        default="curated",
        help="traffic profile (default: curated). Use 'deterministic' for UI tests, 'random' for varied traffic.",
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
        help="tick interval in ms for curated/deterministic profiles (default: 300, fast: 50)",
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
    p.add_argument(
        "--cycles", type=int, default=None,
        help="number of traffic ticks to send (default: 20 for curated, 100 for test; 0 = infinite like --continuous)",
    )
    p.add_argument(
        "--continuous", action="store_true",
        help="restart traffic processes automatically when they exit (keeps server alive for testing)",
    )
