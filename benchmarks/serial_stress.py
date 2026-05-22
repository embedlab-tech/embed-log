"""
serial_stress — cross-platform backend benchmark for embed-log.

Simulates multiple UART-like ports via pyserial socket:// URLs,
stresses the backend, and reports whether frames were lost/duplicated/delayed.

Usage:
    python benchmarks/serial_stress.py --sources 4 --duration 60 --line-rate 1000 --mode disk-only
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

try:
    import yaml
except ImportError:
    yaml = None  # type: ignore[assignment]

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(levelname)-8s  %(message)s",
    datefmt="%H:%M:%S",
)
logger = logging.getLogger("serial_stress")


# ---------------------------------------------------------------------------
#  Defaults
# ---------------------------------------------------------------------------

DEFAULT_BAUD = 921600
DEFAULT_STARTUP_TIMEOUT = 15.0
DEFAULT_SHUTDOWN_TIMEOUT = 10.0


# ---------------------------------------------------------------------------
#  Temp config generation
# ---------------------------------------------------------------------------

def generate_config(
    logs_root: Path,
    sources: list[tuple[str, int]],       # (name, port)
    inject_ports: dict[str, int],
    forward_ports: dict[str, list[int]],
    ws_port: int,
    baudrate: int,
    host: str = "127.0.0.1",
) -> dict:
    """Generate a config dict that can be serialized to YAML."""
    source_entries = []
    for name, port in sources:
        entry: dict = {
            "name": name,
            "type": "uart",
            "port": f"socket://{host}:{port}",
            "baudrate": baudrate,
        }
        if name in inject_ports:
            entry["inject_port"] = inject_ports[name]
        if name in forward_ports and forward_ports[name]:
            entry["forward_ports"] = forward_ports[name]
        source_entries.append(entry)

    # Build tabs with at most 2 panes per tab (config loader constraint)
    src_names = [src_name for src_name, _ in sources]
    tabs = []
    for i in range(0, len(src_names), 2):
        chunk = src_names[i:i + 2]
        label = f"Tab{i // 2}"
        tabs.append({"label": label, "panes": chunk})

    config = {
        "version": 1,
        "server": {
            "host": host,
            "ws_port": ws_port,
            "verbosity": "events",
        },
        "logs": {
            "dir": str(logs_root),
        },
        "baudrate": baudrate,
        "sources": source_entries,
        "tabs": tabs,
    }
    return config


def write_config(path: Path, config: dict) -> None:
    """Serialize config dict to YAML."""
    if yaml is None:
        raise RuntimeError("PyYAML is required — install with: pip install pyyaml")
    path.write_text(yaml.safe_dump(config, default_flow_style=False, sort_keys=False), encoding="utf-8")
    logger.info("wrote temp config: %s", path)


# ---------------------------------------------------------------------------
#  Argument parsing
# ---------------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Cross-platform embed-log backend stress benchmark.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Examples:\n"
            "  # Smoke test — 1 source, 10 seconds, 100 lines/sec\n"
            "  python benchmarks/serial_stress.py --sources 1 --duration 10 --line-rate 100\n\n"
            "  # Four-source baseline\n"
            "  python benchmarks/serial_stress.py --sources 4 --duration 60 --line-rate 1000"
        ),
    )

    parser.add_argument(
        "--sources", type=int, default=4,
        help="Number of simulated UART sources (default: 4)",
    )
    parser.add_argument(
        "--duration", type=int, default=60,
        help="Test duration in seconds (default: 60)",
    )
    parser.add_argument(
        "--line-rate", type=int, default=1000,
        help="Target lines/sec per source (default: 1000)",
    )
    parser.add_argument(
        "--payload-bytes", type=int, default=80,
        help="Payload field size in bytes per line (default: 80)",
    )
    parser.add_argument(
        "--mode",
        choices=["disk-only", "ws-server-no-client"],
        default="disk-only",
        help="Benchmark mode (default: disk-only)",
    )
    parser.add_argument(
        "--baud", type=int, default=DEFAULT_BAUD,
        help=f"Informational baudrate value (default: {DEFAULT_BAUD})",
    )
    parser.add_argument(
        "--logs-root", type=str, default=".benchmark-runs",
        help="Root directory for benchmark session logs (default: .benchmark-runs)",
    )
    parser.add_argument(
        "--report", type=str, default=None,
        help="Write JSON report to this path (default: <logs-root>/report.json)",
    )
    parser.add_argument(
        "--keep-temp", action="store_true",
        help="Do not delete generated config/temp files on exit",
    )
    parser.add_argument(
        "--startup-timeout", type=float, default=DEFAULT_STARTUP_TIMEOUT,
        help=f"Backend startup timeout in seconds (default: {DEFAULT_STARTUP_TIMEOUT})",
    )
    parser.add_argument(
        "--shutdown-timeout", type=float, default=DEFAULT_SHUTDOWN_TIMEOUT,
        help=f"Backend shutdown timeout in seconds (default: {DEFAULT_SHUTDOWN_TIMEOUT})",
    )
    parser.add_argument(
        "--drain-wait", type=float, default=1.0,
        help="Seconds to wait after stopping producers before SIGINT "
             "to let the backend drain in-flight frames (default: 1.0)",
    )

    return parser


# ---------------------------------------------------------------------------
#  Backend subprocess management
# ---------------------------------------------------------------------------

class OutputCollector:
    """
    Background thread that reads lines from a pipe into a bounded deque.
    Works on macOS, Linux, and Windows (no platform-specific select calls).
    """

    def __init__(self, pipe, maxlen: int = 10000):
        self._pipe = pipe
        self.lines: deque[str] = deque(maxlen=maxlen)
        self.full_text: list[str] = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        try:
            for line in iter(self._pipe.readline, ""):
                stripped = line.rstrip()
                self.lines.append(stripped)
                self.full_text.append(stripped)
                if self._stop.is_set():
                    break
        except (OSError, ValueError):
            pass

    def stop(self) -> None:
        self._stop.set()

    def tail(self, n: int = 500) -> str:
        """Return the last *n* characters as a single string."""
        recent = list(self.lines)[-n:]
        return "\n".join(recent)


class BackendProcess:
    """Manages the embed-log backend as a subprocess."""

    def __init__(self, config_path: Path, shutdown_timeout: float = DEFAULT_SHUTDOWN_TIMEOUT):
        self.config_path = config_path
        self.shutdown_timeout = shutdown_timeout
        self._process: Optional[subprocess.Popen] = None
        self._stdout_collector: Optional[OutputCollector] = None
        self._stderr_collector: Optional[OutputCollector] = None

    def start(self, startup_timeout: float = DEFAULT_STARTUP_TIMEOUT) -> None:
        """Launch the backend subprocess and wait for it to be ready."""
        cmd = [sys.executable, "-u", "-m", "backend.cli", "run", "--config", str(self.config_path)]
        logger.info("starting backend: %s", " ".join(cmd))

        self._process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

        # Start background readers so output is captured on all platforms
        if self._process.stdout:
            self._stdout_collector = OutputCollector(self._process.stdout)
        if self._process.stderr:
            self._stderr_collector = OutputCollector(self._process.stderr)

        # Wait for the backend to log its "running" message or timeout
        started_at = time.monotonic()
        ready = False
        while time.monotonic() - started_at < startup_timeout:
            if self._process.poll() is not None:
                raise RuntimeError(
                    f"backend exited prematurely (rc={self._process.returncode})\n"
                    f"stderr: {self._stderr_tail(200)}"
                )
            # Check buffered stderr for the ready message
            if self._stderr_collector:
                for line in self._stderr_collector.lines:
                    if "log server running" in line:
                        ready = True
                        break
            if ready:
                break
            time.sleep(0.1)

        if not ready:
            raise RuntimeError(
                f"backend did not become ready within {startup_timeout}s\n"
                f"stderr so far: {self._stderr_tail(200)}"
            )

        logger.info("backend is ready")

    def _stderr_tail(self, n: int = 500) -> str:
        if self._stderr_collector:
            return self._stderr_collector.tail(n)
        return ""

    def stop(self) -> int:
        """Send SIGINT/SIGTERM to the backend and wait for it to exit."""
        if self._process is None:
            return -1

        if self._process.poll() is not None:
            return self._process.returncode

        # Send SIGINT first (graceful shutdown)
        logger.info("sending SIGINT to backend (pid=%d)", self._process.pid)
        if sys.platform == "win32":
            self._process.terminate()
        else:
            os.kill(self._process.pid, signal.SIGINT)

        try:
            self._process.wait(timeout=self.shutdown_timeout)
            logger.info("backend exited with rc=%d", self._process.returncode)
        except subprocess.TimeoutExpired:
            logger.warning("backend did not exit in %.1fs, sending SIGKILL", self.shutdown_timeout)
            self._process.kill()
            self._process.wait(timeout=5.0)
            logger.warning("backend killed (rc=%d)", self._process.returncode)

        return self._process.returncode

    @property
    def returncode(self) -> Optional[int]:
        if self._process is None:
            return None
        return self._process.returncode

    @property
    def stdout_tail(self) -> str:
        if self._stdout_collector:
            return self._stdout_collector.tail()
        return ""

    @property
    def stderr_tail(self) -> str:
        if self._stderr_collector:
            return self._stderr_collector.tail()
        return ""


# ---------------------------------------------------------------------------
#  Frame format
# ---------------------------------------------------------------------------

# Each generated frame looks like:
#   BENCH src=SRC0 seq=000000001 t_ns=184467440000000000 payload=xxxxxxxx
BENCHMARK_LINE_PREFIX = "BENCH "


def make_bench_frame(src: str, seq: int, payload: str) -> str:
    """Build a single benchmark frame string (no trailing newline)."""
    t_ns = time.time_ns()
    return f"BENCH src={src} seq={seq:09d} t_ns={t_ns} payload={payload}"


# ---------------------------------------------------------------------------
#  Virtual UART producers
# ---------------------------------------------------------------------------

@dataclass
class ProducerStats:
    """Per-producer statistics collected during a benchmark run."""
    generated: int = 0
    sent: int = 0
    send_blocked_sec: float = 0.0
    produce_start_time: Optional[float] = None  # monotonic time when production began


class VirtualUartProducer:
    """
    TCP server that simulates a UART source.

    Listens on a port, waits for the embed-log backend to connect
    (via pyserial socket://), then generates numbered BENCH frames at
    the configured line rate using monotonic clock pacing.
    """

    def __init__(
        self,
        name: str,
        port: int,
        line_rate: int,
        payload_bytes: int,
    ):
        self.name = name
        self.port = port
        self.line_rate = line_rate
        self.payload_bytes = payload_bytes
        self.stats = ProducerStats()

        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", port))
        self._sock.listen(1)
        self._sock.settimeout(0.5)  # allow periodic _stop checks

        self._conn: Optional[socket.socket] = None
        self._connected = threading.Event()
        self._start_producing = threading.Event()
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None

    def start(self) -> None:
        """Start the listening thread (non-blocking)."""
        self._thread = threading.Thread(
            target=self._run,
            daemon=True,
            name=f"prod-{self.name}",
        )
        self._thread.start()

    def _run(self) -> None:
        """Accept one backend connection, then produce frames until stopped."""
        # ---- Accept connection ----
        while not self._stop.is_set():
            try:
                conn, addr = self._sock.accept()
                logger.info("[%s] backend connected from %s", self.name, addr)
                self._conn = conn
                # Use a generous send timeout so backpressure doesn't break the connection;
                # only true connection errors are treated as lost.
                self._conn.settimeout(10.0)
                self._connected.set()
                break
            except socket.timeout:
                continue

        # ---- Wait for start signal (all producers connected) ----
        self._start_producing.wait()
        self.stats.produce_start_time = time.monotonic()

        # ---- Produce frames ----
        interval = 1.0 / self.line_rate  # seconds between lines
        next_send = time.monotonic()

        # Pre-compute a payload string of the requested size
        payload = "x" * self.payload_bytes

        while not self._stop.is_set():
            now = time.monotonic()

            # Pace: sleep if we're ahead of schedule
            if now < next_send:
                # Brief sleep; don't oversleep on short intervals
                remaining = next_send - now
                if remaining > 0.001:
                    time.sleep(min(remaining, 0.01))
                continue

            seq = self.stats.generated + 1
            line = make_bench_frame(self.name, seq, payload) + "\n"
            data = line.encode("utf-8")
            self.stats.generated += 1

            # Send — treat timeouts as backpressure (not lost connection)
            try:
                if self._conn is not None:
                    before = time.monotonic()
                    self._conn.sendall(data)
                    self.stats.sent += 1
                    elapsed = time.monotonic() - before
                    # Track wall time spent blocked on send
                    if elapsed > 0.001:  # more than 1ms = some backpressure
                        self.stats.send_blocked_sec += elapsed
            except socket.timeout:
                # Backpressure: backend not reading fast enough
                self.stats.send_blocked_sec += 10.0  # socket timeout duration
            except (ConnectionError, OSError) as exc:
                # Genuine connection failure
                self.stats.send_blocked_sec += time.monotonic() - next_send
                logger.warning("[%s] connection lost: %s", self.name, exc)
                break

            next_send += interval

            # If we fell behind more than 1s, reset schedule to avoid catch-up burst
            behind = time.monotonic() - next_send
            if behind > 1.0:
                next_send = time.monotonic() + interval

    def wait_connected(self, timeout: float = 30.0) -> bool:
        """Block until the backend has connected to this producer."""
        return self._connected.wait(timeout=timeout)

    def start_producing(self) -> None:
        """Signal this producer to begin sending frames."""
        self._start_producing.set()

    def stop(self) -> None:
        """Signal stop and close resources."""
        self._stop.set()
        if self._conn is not None:
            try:
                self._conn.close()
            except OSError:
                pass
        if self._thread is not None and self._thread.is_alive():
            self._thread.join(timeout=2.0)

    def close(self) -> None:
        """Release the listening socket."""
        self.stop()
        try:
            self._sock.close()
        except OSError:
            pass


class ProducerPool:
    """Manages a collection of VirtualUartProducer instances."""

    def __init__(
        self,
        sources: list[tuple[str, int]],
        line_rate: int,
        payload_bytes: int,
    ):
        self.producers: list[VirtualUartProducer] = []
        for name, port in sources:
            self.producers.append(
                VirtualUartProducer(
                    name=name,
                    port=port,
                    line_rate=line_rate,
                    payload_bytes=payload_bytes,
                )
            )

    def start_all(self) -> None:
        """Start all producer TCP listeners."""
        for p in self.producers:
            p.start()
        logger.info(
            "started %d virtual UART producers on ports %s",
            len(self.producers),
            [p.port for p in self.producers],
        )

    def wait_all_connected(self, timeout: float = 30.0) -> bool:
        """Block until every producer has an active backend connection."""
        ok = True
        for p in self.producers:
            if not p.wait_connected(timeout=timeout):
                logger.error("[%s] backend did not connect within %.1fs", p.name, timeout)
                ok = False
        if ok:
            logger.info("all producers have active backend connections")
        return ok

    def start_all_producing(self) -> None:
        """Signal all producers to begin sending frames."""
        for p in self.producers:
            p.start_producing()

    def stop_all(self) -> None:
        """Stop all producers."""
        for p in self.producers:
            p.stop()

    def close_all(self) -> None:
        """Close all producers (stop + release sockets)."""
        for p in self.producers:
            p.close()

    def collect_stats(self) -> dict[str, dict]:
        """Return per-source stats dict."""
        per_source = {}
        total_generated = 0
        total_sent = 0
        for p in self.producers:
            s = p.stats
            per_source[p.name] = {
                "generated": s.generated,
                "sent": s.sent,
                "send_blocked_sec": round(s.send_blocked_sec, 4),
            }
            total_generated += s.generated
            total_sent += s.sent
        return {
            "per_source": per_source,
            "total_generated": total_generated,
            "total_sent": total_sent,
        }


# ---------------------------------------------------------------------------
#  Log verifier
# ---------------------------------------------------------------------------

# Regex to extract source name and sequence number from a logged benchmark frame.
# The line may have a backend timestamp prefix like "[2026-05-22T19:51:03...] ".
BENCH_FRAME_RE = re.compile(r'BENCH src=(\S+) seq=(\d{9}) t_ns=\d+ payload=')


# Session directory names follow the pattern: YYYY-MM-DD_HH-MM-SS[__job_id]
_SESSION_DIR_RE = re.compile(r'^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}')


def find_latest_session_dir(logs_root: Path) -> Optional[Path]:
    """Return the most recently created session subdirectory under *logs_root*."""
    dirs = [d for d in logs_root.iterdir() if d.is_dir() and _SESSION_DIR_RE.match(d.name)]
    if not dirs:
        return None
    # Directory names are ISO-like timestamps so sort by name = sort by time
    return max(dirs, key=lambda d: d.name)


class LogVerifier:
    """
    Parses session log files, extracts benchmark frames, and reports
    missing, duplicate, out-of-order, and corrupt frame counts.
    """

    def __init__(self, session_dir: Path, source_names: list[str]):
        self.session_dir = Path(session_dir)
        self.source_logs: dict[str, Path] = {}
        self._discover_logs(source_names)

    def _discover_logs(self, source_names: list[str]) -> None:
        """Find the log file for each known source name."""
        for name in source_names:
            candidates = list(self.session_dir.glob(f"*__{name}__*.log"))
            if candidates:
                self.source_logs[name] = candidates[0]
            else:
                logger.warning("[%s] no log file found in %s", name, self.session_dir)

    def verify_source(self, source_name: str, expected_count: int) -> dict:
        """
        Verify a single source's log file.

        *expected_count* is the number of frames the producer says it generated
        (i.e. seq = 1 … expected_count).

        Returns a dict with keys: logged, missing, duplicates, out_of_order, corrupt.
        """
        log_file = self.source_logs.get(source_name)
        if log_file is None or not log_file.is_file():
            return {"logged": 0, "missing": expected_count, "duplicates": 0,
                    "out_of_order": 0, "corrupt": 0}

        seen_seqs: list[int] = []
        corrupt = 0

        for raw_line in log_file.read_text(encoding="utf-8").splitlines():
            m = BENCH_FRAME_RE.search(raw_line)
            if m:
                src = m.group(1)
                seq = int(m.group(2))
                if src == source_name:
                    seen_seqs.append(seq)
            elif "BENCH" in raw_line:
                # Line mentions BENCH but doesn't match → malformed
                corrupt += 1

        if not seen_seqs:
            return {"logged": 0, "missing": expected_count, "duplicates": 0,
                    "out_of_order": 0, "corrupt": corrupt}

        # Track duplicates and out-of-order
        seen_set: set[int] = set()
        duplicates = 0
        out_of_order = 0
        prev_seq = -1

        for seq in seen_seqs:
            if seq in seen_set:
                duplicates += 1
            else:
                seen_set.add(seq)
            if prev_seq > 0 and seq < prev_seq:
                out_of_order += 1
            prev_seq = seq

        # Frames we can account for: unique seq values in [min, max]
        unique_seqs = len(seen_set)
        expected = set(range(1, expected_count + 1))
        actual = seen_set & expected  # only count seqs within expected range
        missing = len(expected - actual)
        logged = len(actual)

        return {
            "logged": logged,
            "missing": missing,
            "duplicates": duplicates,
            "out_of_order": out_of_order,
            "corrupt": corrupt,
        }

    def verify_all(
        self,
        expected_counts: dict[str, int],
    ) -> tuple[dict[str, dict], dict[str, int]]:
        """
        Verify all source log files.

        Returns (per_source, totals) where totals aggregates
        logged, missing, duplicates, corrupt.
        """
        per_source: dict[str, dict] = {}
        totals: dict[str, int] = {"logged": 0, "missing": 0,
                                   "duplicates": 0, "corrupt": 0}

        for src_name, exp_cnt in expected_counts.items():
            stats = self.verify_source(src_name, exp_cnt)
            per_source[src_name] = stats
            for k in ("logged", "missing", "duplicates", "corrupt"):
                totals[k] += stats[k]

        return per_source, totals


# ---------------------------------------------------------------------------
#  Main benchmark orchestrator
# ---------------------------------------------------------------------------

def allocate_ports(sources_count: int) -> list[int]:
    """
    Allocate *sources_count* free TCP ports by binding ephemeral sockets.
    Sockets are closed immediately so ports may be re-used (acceptable for
    sequential benchmark runs on localhost).
    """
    ports: list[int] = []
    for _ in range(sources_count):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("127.0.0.1", 0))
            ports.append(s.getsockname()[1])
    return ports


def run_benchmark(args: argparse.Namespace) -> dict:
    """Run the benchmark and return a report dict."""
    logs_root = Path(args.logs_root)
    logs_root.mkdir(parents=True, exist_ok=True)

    source_names = [f"SRC{i}" for i in range(args.sources)]
    source_ports = allocate_ports(args.sources)
    sources = list(zip(source_names, source_ports))

    ws_port = 0  # will be overridden based on mode
    if args.mode == "ws-server-no-client":
        # Bind to an ephemeral port to enable the WS server without a client
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("127.0.0.1", 0))
            ws_port = s.getsockname()[1]

    # --- Start virtual UART producers ---
    pool = ProducerPool(
        sources=sources,
        line_rate=args.line_rate,
        payload_bytes=args.payload_bytes,
    )
    pool.start_all()

    # --- Generate temp config ---
    config = generate_config(
        logs_root=logs_root,
        sources=sources,
        inject_ports={},
        forward_ports={},
        ws_port=ws_port,
        baudrate=args.baud,
    )

    config_dir = tempfile.mkdtemp(prefix="embed-log-bench-")
    config_path = Path(config_dir) / "benchmark-config.yml"
    write_config(config_path, config)

    # --- Start backend ---
    backend = BackendProcess(
        config_path=config_path,
        shutdown_timeout=args.shutdown_timeout,
    )

    try:
        backend.start(startup_timeout=args.startup_timeout)

        # --- Wait for all backend connections ---
        if not pool.wait_all_connected(timeout=args.startup_timeout):
            raise RuntimeError("not all producers have active backend connections")

        # --- Begin measurement: signal all producers to start ---
        pool.start_all_producing()
        logger.info(
            "benchmark running: sources=%d duration=%ds rate=%d lps/source mode=%s",
            args.sources, args.duration, args.line_rate, args.mode,
        )
        time.sleep(args.duration)

        # --- Stop producers (stop sending) ---
        pool.stop_all()

        # --- Drain: give backend time to flush queued frames before SIGINT ---
        if args.drain_wait > 0:
            logger.info("drain wait %.1fs…", args.drain_wait)
            time.sleep(args.drain_wait)

        # --- Collect producer stats ---
        pool_stats = pool.collect_stats()

        # --- Stop backend ---
        returncode = backend.stop()

        # --- Verify session log files ---
        session_dir = find_latest_session_dir(logs_root)
        verifier_per_source: dict[str, dict] = {}
        verifier_totals: dict[str, int] = {"logged": 0, "missing": 0, "duplicates": 0, "corrupt": 0}

        if session_dir is not None:
            logger.info("verifying logs in %s", session_dir)
            # Verify against frames *actually sent* over TCP (not just generated).
            # If generated != sent, the producer-side backpressure is reported
            # as send_blocked_sec in per_source stats.
            expected_counts = {
                p.name: p.stats.sent for p in pool.producers
            }
            verifier = LogVerifier(session_dir, source_names)
            verifier_per_source, verifier_totals = verifier.verify_all(expected_counts)
            logger.info(
                "verification: logged=%d missing=%d duplicates=%d corrupt=%d",
                verifier_totals["logged"], verifier_totals["missing"],
                verifier_totals["duplicates"], verifier_totals["corrupt"],
            )
        else:
            logger.warning("no session directory found under %s", logs_root)

        # --- Merge producer stats with verifier stats per source ---
        merged_per_source: dict[str, dict] = {}
        for p in pool.producers:
            src_name = p.name
            prod = pool_stats["per_source"].get(src_name, {})
            ver = verifier_per_source.get(src_name, {})
            merged_per_source[src_name] = {
                "generated": prod.get("generated", 0),
                "sent": prod.get("sent", 0),
                "logged": ver.get("logged", 0),
                "missing": ver.get("missing", 0),
                "duplicates": ver.get("duplicates", 0),
                "out_of_order": ver.get("out_of_order", 0),
                "corrupt": ver.get("corrupt", 0),
                "send_blocked_sec": prod.get("send_blocked_sec", 0.0),
            }

        # ok = backend clean exit AND no frame issues
        ok = (
            returncode == 0
            and verifier_totals["missing"] == 0
            and verifier_totals["duplicates"] == 0
            and verifier_totals["corrupt"] == 0
        )
        report = {
            "ok": ok,
            "mode": args.mode,
            "ws_port": ws_port,
            "sources": args.sources,
            "duration_sec": args.duration,
            "line_rate_per_source": args.line_rate,
            "payload_bytes": args.payload_bytes,
            "backend": {
                "returncode": returncode,
                "session_dir": str(session_dir) if session_dir else "",
                "stdout_tail": "",
                "stderr_tail": "",
            },
            "totals": {
                "generated": pool_stats["total_generated"],
                "sent": pool_stats["total_sent"],
                "logged": verifier_totals["logged"],
                "missing": verifier_totals["missing"],
                "duplicates": verifier_totals["duplicates"],
                "corrupt": verifier_totals["corrupt"],
            },
            "per_source": merged_per_source,
        }

        # Collect backend output tails
        report["backend"]["stdout_tail"] = backend.stdout_tail
        report["backend"]["stderr_tail"] = backend.stderr_tail

        return report

    except Exception:
        logger.exception("benchmark failed")
        raise

    finally:
        # Always stop producers and clean up
        pool.close_all()
        try:
            if backend.returncode is None:
                backend.stop()
        except Exception:
            pass
        if not args.keep_temp:
            import shutil
            shutil.rmtree(config_dir, ignore_errors=True)
            logger.info("cleaned up temp config: %s", config_dir)
        else:
            logger.info("temp config kept at: %s", config_path)


def print_summary(report: dict) -> None:
    """Print a concise terminal summary."""
    status = "PASS" if report["ok"] else "FAIL"
    t = report["totals"]
    print()
    print(
        f"{status} {report['mode']} "
        f"sources={report['sources']} "
        f"duration={report['duration_sec']}s "
        f"rate={report['line_rate_per_source']} lps/source"
    )
    print(
        f"generated={t['generated']} sent={t['sent']} logged={t['logged']} "
        f"missing={t['missing']} duplicates={t['duplicates']} corrupt={t['corrupt']}"
    )
    print(f"report={report.get('report_path', '')}")
    print()


# ---------------------------------------------------------------------------
#  Entry point
# ---------------------------------------------------------------------------

def main(argv: Optional[list[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if yaml is None:
        print("ERROR: PyYAML is required. Install with: pip install pyyaml", file=sys.stderr)
        return 1

    try:
        report = run_benchmark(args)
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    report_path = args.report or str(Path(args.logs_root) / "report.json")
    report["report_path"] = report_path

    import json
    Path(report_path).parent.mkdir(parents=True, exist_ok=True)
    Path(report_path).write_text(
        json.dumps(report, indent=2, default=str),
        encoding="utf-8",
    )

    print_summary(report)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
