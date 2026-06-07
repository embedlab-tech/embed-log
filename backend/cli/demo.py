"""Embed-log demo subcommand — port of run_demo.sh."""

from __future__ import annotations

import argparse
import os
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

try:
    import termios
    _HAS_TERMIOS = hasattr(termios, 'TCSANOW') and hasattr(os, 'openpty')
except ImportError:
    _HAS_TERMIOS = False

import yaml


# Ports used by the demo setup
INJECT_PORTS = [5001, 5002, 5003, 5004, 5005, 5006, 5007]
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
        if content_mode == "curated":
            return 20
        if content_mode == "test":
            return 100
        return 100

    def run(self) -> int:
        """Start the demo and wait for it to finish."""
        demo_config = _resolve_demo_config()
        if demo_config is None:
            print("Demo config not found.", file=sys.stderr)
            print("Ensure embed-log.demo.yml is bundled or run from the repository root.", file=sys.stderr)
            return 1

        if self.args.print_config:
            print(demo_config.read_text("utf-8"))
            return 0

        # ── Port cleanup ──
        print("Checking demo ports...")
        for p in ALL_PORTS:
            proto = "udp" if p in UDP_PORTS else "tcp"
            _free_port(p, proto)
        # ── Create virtual UART PTY pairs and generate temp config ──
        self._uart_slave_fds: list[int] = []
        self._uart_master_fds: dict[str, int] = {}
        self._uart_names: list[str] = []
        try:
            raw = demo_config.read_text("utf-8")
        except OSError as exc:
            print(f"ERROR: cannot read demo config {demo_config}: {exc}", file=sys.stderr)
            return 1

        if _HAS_TERMIOS:
            for name, placeholder in [("UART_DUT", "__uart_dut_placeholder__"),
                                       ("UART_DEBUG", "__uart_debug_placeholder__")]:
                try:
                    master_fd, slave_fd = os.openpty()
                    attr = termios.tcgetattr(slave_fd)
                    attr[0] = attr[0] & ~termios.BRKINT
                    attr[3] = attr[3] & ~(termios.ECHO | termios.ICANON | termios.ISIG)
                    termios.tcsetattr(slave_fd, termios.TCSANOW, attr)
                    slave_path = os.ttyname(slave_fd)
                    self._uart_slave_fds.append(slave_fd)
                    self._uart_master_fds[name] = master_fd
                    self._uart_names.append(name)
                    raw = raw.replace(placeholder, slave_path)
                    print(f"  UART {name} → {slave_path}")
                except OSError as exc:
                    print(f"  UART {name}: could not create PTY — {exc}", file=sys.stderr)
        else:
            print("  UART: PTY support not available on this platform — skipping UART sources")
            # Remove UART sources and UART tab from config
            import re
            raw = re.sub(r'(?s)\n  # ── UART sources.*?\n  - label: UART\n    panes:.*?(?=\n\s*\n|\Z)', '', raw)
            raw += "\n"  # ensure trailing newline

        # Write temp config with resolved UART paths
        tmp = tempfile.NamedTemporaryFile(mode="w", suffix=".yml", delete=False, dir=demo_config.parent)
        tmp.write(raw)
        tmp.close()
        demo_config = Path(tmp.name)

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
        server = subprocess.Popen(server_cmd, start_new_session=True)
        self._server_pid = server.pid
        self._processes.append((server, None))

        time.sleep(1)
        if server.poll() is not None:
            print("ERROR: embed-log server failed to start.", file=sys.stderr)
            self._cleanup()
            return 1
        # ── Resolve defaults with --fast ──
        # ── Resolve defaults ──
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
                self.args.interval_min = 10.0
            if self.args.interval_max is None:
                self.args.interval_max = 30.0
            if self.args.inject_interval is None:
                self.args.inject_interval = 10.0
            if self.args.tick_ms is None:
                self.args.tick_ms = 500

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

        self._install_signal_handlers()

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
        self._processes.append((subprocess.Popen(cmd, start_new_session=True), cmd))

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
            self._processes.append((subprocess.Popen(inject_cmd, start_new_session=True), inject_cmd))

        deterministic_script = DEMO_UTILS / "deterministic_demo_traffic.py"
        if deterministic_script.is_file():
            cbor_cmd = [
                sys.executable, str(deterministic_script),
                "--cbor",
                "--udp", "SENSOR_CBOR=127.0.0.1:6003",
                "--tick-ms", str(self.args.tick_ms),
                "--cycles", str(self._content_cycles("curated")),
            ]
            print(f"Starting CBOR demo traffic (tick {self.args.tick_ms}ms)...")
            self._processes.append((subprocess.Popen(cbor_cmd, start_new_session=True), cbor_cmd))

    def _start_deterministic_traffic(self, content_mode: str = "test") -> None:
        """Start deterministic demo traffic (test or curated content)."""
        deterministic_script = DEMO_UTILS / "deterministic_demo_traffic.py"
        if not deterministic_script.is_file():
            print(f"Traffic script not found: {deterministic_script}", file=sys.stderr)
            return

        cycles = self._content_cycles(content_mode)
        tick_ms = self.args.tick_ms
        label = "curated demo" if content_mode == "curated" else "deterministic test"

        # Main traffic: UDP sources
        main_targets = [
            "SENSOR_A=127.0.0.1:6000",
            "SENSOR_B=127.0.0.1:6001",
            "SENSOR_C=127.0.0.1:6002",
            "SENSOR_D=127.0.0.1:6004",
            "SENSOR_COAP=127.0.0.1:6005",
        ]
        main_cmd = [
            sys.executable, str(deterministic_script),
            "--content", content_mode,
            "--tick-ms", str(tick_ms),
            "--cycles", str(cycles),
        ]
        for target in main_targets:
            main_cmd.extend(["--udp", target])
        print(f"Starting {label} traffic (tick {tick_ms}ms)...")
        self._processes.append((subprocess.Popen(main_cmd, start_new_session=True), main_cmd))

        # CBOR traffic: one additional CBOR source
        cbor_cmd = [
            sys.executable, str(deterministic_script),
            "--content", content_mode,
            "--cbor",
            "--udp", "SENSOR_CBOR=127.0.0.1:6003",
            "--tick-ms", str(tick_ms),
            "--cycles", str(cycles),
        ]
        print(f"Starting CBOR {label} traffic (tick {tick_ms}ms)...")
        self._processes.append((subprocess.Popen(cbor_cmd, start_new_session=True), cbor_cmd))

        # UART traffic (written directly to master PTY fds)
        if self._uart_master_fds:
            uart_thread = threading.Thread(
                target=self._run_uart_traffic,
                args=(content_mode,),
                daemon=True,
                name="demo-uart",
            )
            uart_thread.start()

    def _run_uart_traffic(self, content_mode: str) -> None:
        """Write deterministic test lines to UART master PTY fds."""
        tick = 0
        seq: dict[str, int] = {name: 1 for name in self._uart_master_fds}
        tick_s = self.args.tick_ms / 1000.0
        try:
            while tick < self._content_cycles(content_mode) or self._content_cycles(content_mode) == 0:
                tick += 1
                time.sleep(tick_s)
                for name, fd in self._uart_master_fds.items():
                    s = seq[name]
                    seq[name] = s + 1
                    line = f"UART {name} tick={tick:03d} seq={s:04d}\n"
                    try:
                        os.write(fd, line.encode("utf-8"))
                    except OSError:
                        pass
        except Exception:
            pass


    def _install_signal_handlers(self) -> None:
        """Ensure cleanup runs on Ctrl+C, Ctrl+Z, and terminal close."""
        def _handler(signum, _frame):
            self._cleanup()
            if signum == signal.SIGTSTP:
                # Re-raise default behaviour so the shell actually suspends us.
                signal.signal(signal.SIGTSTP, signal.SIG_DFL)
                os.kill(os.getpid(), signal.SIGTSTP)
            else:
                sys.exit(0)

        signal.signal(signal.SIGINT, _handler)
        signal.signal(signal.SIGTERM, _handler)
        signal.signal(signal.SIGTSTP, _handler)
        if hasattr(signal, 'SIGHUP'):
            signal.signal(signal.SIGHUP, _handler)

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
            except (ProcessLookupError, ChildProcessError, subprocess.TimeoutExpired):
                pass
        self._processes.clear()
        # Close UART PTY fds
        for fd in getattr(self, "_uart_slave_fds", []):
            try:
                os.close(fd)
            except OSError:
                pass
        for fd in getattr(self, "_uart_master_fds", {}).values():
            try:
                os.close(fd)
            except OSError:
                pass


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
            "  embed-log demo --fast --continuous\n"
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
        "--print-config", action="store_true",
        help="print the demo config and exit (does not start the server)",
    )
    p.add_argument(
        "--fast", action="store_true",
        help="use faster intervals for demos and testing (tick 50ms)",
    )
    p.add_argument(
        "--tick-ms", type=int, default=None,
        help="tick interval in ms for curated/deterministic profiles (default: 500, fast: 50)",
    )
    p.add_argument(
        "--interval-min", type=float, default=None,
        help="random profile minimum interval in seconds (default: 10, fast: 0.1)",
    )
    p.add_argument(
        "--interval-max", type=float, default=None,
        help="random profile maximum interval in seconds (default: 30, fast: 0.3)",
    )
    p.add_argument(
        "--inject-interval", type=float, default=None,
        help="random profile inject interval in seconds (default: 10, fast: 1)",
    )
    p.add_argument(
        "--cycles", type=int, default=None,
        help="number of traffic ticks to send (default: 20 for curated, 100 for test; 0 = infinite like --continuous)",
    )
    p.add_argument(
        "--continuous", action="store_true",
        help="restart traffic processes automatically when they exit (keeps server alive for testing)",
    )
