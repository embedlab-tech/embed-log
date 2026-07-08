"""
End-to-end tests for embed-log Rust backend + Python SDK.

These tests start a real embed-log server process with a temporary config,
exercise the full SDK, watcher, and CLI, then tear down the server.

Prerequisites:
    - embed-log binary must be built (``cargo build``)
    - SDK must be installed (``pip install -e sdk/python``)
"""

from __future__ import annotations

import json
import os
import pty
import select
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from typing import Generator, Optional

import pytest

# ---- Helpers -----------------------------------------------------------


def free_port() -> int:
    """Return a free TCP port on localhost."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.fixture(scope="session")
def embed_log_binary() -> Path:
    """Path to the embed-log binary."""
    repo_root = Path(__file__).resolve().parent.parent.parent.parent
    candidates = [
        repo_root / "target" / "debug" / "embed-log",
        repo_root / "target" / "release" / "embed-log",
    ]
    for c in candidates:
        if c.exists():
            return c
    pytest.skip("embed-log binary not built; run 'cargo build' first")


@pytest.fixture(scope="session")
def frontend_dir() -> Path:
    repo_root = Path(__file__).resolve().parent.parent.parent.parent
    fd = repo_root / "frontend"
    if fd.exists():
        return fd
    pytest.skip("frontend directory not found")


@pytest.fixture
def temp_dir() -> Generator[Path, None, None]:
    with tempfile.TemporaryDirectory(prefix="embed-log-e2e-") as d:
        yield Path(d)


@pytest.fixture
def pty_pair() -> Generator[tuple[int, str], None, None]:
    """Create a PTY pair. Returns (master_fd, slave_name).
    The slave fd is closed so the embed-log server can open it."""
    master_fd, slave_fd = pty.openpty()
    slave_name = os.ttyname(slave_fd)
    os.close(slave_fd)
    try:
        yield (master_fd, slave_name)
    finally:
        os.close(master_fd)


# ---- E2E server fixture -------------------------------------------------


class E2eServer:
    """Manages a embed-log server process for testing."""

    def __init__(self, binary: Path, config_path: Path, frontend: Path,
                 log_dir: Path, host: str, port: int):
        self.binary = binary
        self.config_path = config_path
        self.frontend = frontend
        self.log_dir = log_dir
        self.host = host
        self.port = port
        self.process: Optional[subprocess.Popen] = None
        self.stdout: list[str] = []

    def start(self) -> None:
        self.process = subprocess.Popen(
            [str(self.binary), "run",
             "--config", str(self.config_path),
             "--frontend-dir", str(self.frontend),
             "--no-open-browser",
             "--host", self.host,
             "--ws-port", str(self.port)],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        )
        deadline = time.time() + 15
        while time.time() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(
                    f"Server exited early (code {self.process.returncode}):\n"
                    + "".join(self.stdout))
            line = self.process.stdout.readline()  # type: ignore
            if line:
                self.stdout.append(line)
                if "UI ready at" in line:
                    return
            time.sleep(0.05)
        self.stop()
        raise RuntimeError("Server did not start within 15s")

    def stop(self) -> None:
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            self.process = None

    def ws_url(self) -> str:
        return f"ws://{self.host}:{self.port}/api/v1/control"


@pytest.fixture
def e2e_server(embed_log_binary: Path, frontend_dir: Path, temp_dir: Path,
               pty_pair: tuple[int, str]) -> Generator[E2eServer, None, None]:
    """Start an embed-log server with a UART PTY source and a UDP source."""
    import yaml

    master_fd, slave_name = pty_pair
    port = free_port()

    config = {
        "version": 1,
        "server": {"host": "127.0.0.1", "ws_port": port,
                    "app_name": "embed-log-e2e", "timestamp_mode": "absolute"},
        "logs": {"dir": str(temp_dir / "logs")},
        "baudrate": 115200,
        "sources": [
            {"name": "DUT_UART", "label": "DUT", "type": "uart", "port": slave_name},
            {"name": "PYTEST", "label": "Pytest", "type": "udp", "port": 0},
        ],
        "tabs": [{"label": "Main", "panes": ["DUT_UART", "PYTEST"]}],
    }
    config_path = temp_dir / "embed-log-e2e.yml"
    config_path.write_text(yaml.dump(config))

    # Companion commands file
    cmd_path = temp_dir / "embed-log-e2e.commands.yml"
    cmd_path.write_text(
        "sources:\n  DUT_UART:\n    - \"help\\r\\n\"\n    - \"version\\r\\n\"\n")

    server = E2eServer(embed_log_binary, config_path, frontend_dir,
                       temp_dir / "logs", "127.0.0.1", port)
    server.start()
    try:
        yield server
    finally:
        server.stop()


# ---- Tests --------------------------------------------------------------


class TestE2eSdkConnect:
    """Phase 8 E2E — SDK connects, injects, TX, subscribes."""

    def test_hello_via_from_config(self, e2e_server: E2eServer):
        """Python SDK connects with from_config() and receives hello.result."""
        from embed_log_sdk import EmbedLogClient

        client = EmbedLogClient.from_config(e2e_server.config_path, origin="e2e")
        try:
            assert client._hello_received
            assert client._session is not None and client._session.id
            assert "DUT_UART" in client._sources
            assert client._sources["DUT_UART"].writable is True
            assert "PYTEST" in client._sources
        finally:
            client.close()

    def test_inject_log_reaches_subscriber_and_file(self, e2e_server: E2eServer):
        """inject_log() creates a log entry visible through subscription
        and written to the source log file."""
        from embed_log_sdk import EmbedLogClient

        with EmbedLogClient(e2e_server.ws_url(), origin="e2e") as client:
            client.subscribe(["PYTEST"])
            client.inject_log("PYTEST", "e2e-test-inject", color="cyan")

            entries = list(client.entries(timeout=3.0))
            injected = [e for e in entries if "e2e-test-inject" in e.message]
            assert len(injected) >= 1
            entry = injected[0]
            assert entry.origin == "e2e"
            assert entry.source_id == "PYTEST"
            assert entry.line_idx >= 0

        # Assert the log file was written
        log_dirs = sorted(e2e_server.log_dir.iterdir())
        assert log_dirs, "no session dir found"
        session_dir = log_dirs[0]
        log_files = list(session_dir.glob("*.log"))
        log_content = ""
        for f in log_files:
            text = f.read_text()
            if "e2e-test-inject" in text:
                log_content = text
                break
        assert "e2e-test-inject" in log_content, \
            f"injected entry not found in log files under {session_dir}"

    def test_tx_write_to_uart_pty(self, e2e_server: E2eServer,
                                   pty_pair: tuple[int, str]):
        """tx_write() writes exact bytes to the PTY and records a TX log entry.

        Uses the non-exclusive serial port fallback added in
        ``open_serial_with_fallback()`` which works on macOS where
        ``TIOCEXCL`` is rejected by PTY slaves with ENOTTY.
        """
        from embed_log_sdk import EmbedLogClient

        master_fd, _ = pty_pair

        with EmbedLogClient(e2e_server.ws_url(), origin="e2e") as client:
            client.subscribe(["DUT_UART"])
            written = client.tx_write("DUT_UART", "version\r\n")
            assert written == 9

            # Read from the master side of the PTY
            deadline = time.time() + 3.0
            data = b""
            while time.time() < deadline and len(data) < 9:
                r, _, _ = select.select([master_fd], [], [], 0.5)
                if r:
                    chunk = os.read(master_fd, 32)
                    if chunk:
                        data += chunk
            assert data == b"version\r\n", f"got {data!r}"

            # Verify TX log entry appears in subscription
            entries = list(client.entries(timeout=2.0))
            tx_entries = [e for e in entries if e.is_tx]
            assert len(tx_entries) >= 1
            tx = tx_entries[0]
            assert tx.origin == "e2e"
            assert tx.source_id == "DUT_UART"
            assert "version" in tx.message

    def test_subscribe_filters_by_source(self, e2e_server: E2eServer):
        """subscribe() receives only requested sources."""
        from embed_log_sdk import EmbedLogClient

        with EmbedLogClient(e2e_server.ws_url(), origin="e2e") as client:
            client.subscribe(["PYTEST"])
            client.inject_log("PYTEST", "pytest-only-msg")

            for e in client.entries(timeout=2.0):
                assert e.source_id == "PYTEST"

    def test_command_suggestions_in_runtime_metadata(self, e2e_server: E2eServer):
        """Companion YAML commands are visible in runtime session config."""
        result = subprocess.run(
            [str(e2e_server.binary), "sessions", "info",
             "--log-dir", str(e2e_server.log_dir),
             sorted(e2e_server.log_dir.iterdir())[0].name,
             "--json"],
            capture_output=True, text=True, timeout=10,
        )
        assert result.returncode == 0
        manifest = json.loads(result.stdout)
        pane_commands = manifest.get("pane_commands", {})
        assert "DUT_UART" in pane_commands, \
            f"pane_commands missing DUT_UART: {pane_commands}"
        assert any("help" in c for c in pane_commands["DUT_UART"]), \
            f"help command not found: {pane_commands['DUT_UART']}"


class TestE2eWatcher:
    """Phase 8 E2E — watcher observes logs, creates markers, CLI."""

    def test_watcher_writes_evidence(self, e2e_server: E2eServer, temp_dir: Path):
        """Watcher observes injected log and writes JSONL evidence."""
        from embed_log_sdk import EmbedLogClient
        from embed_log_sdk.watcher import Watcher, WatcherConfig, WatchRule

        evidence = temp_dir / "evidence.jsonl"
        wconfig = WatcherConfig(
            server_url=e2e_server.ws_url(), output_path=evidence,
            rules=[WatchRule(name="detect", sources=["PYTEST"],
                             pattern="e2e-watch-evidence", marker=False)])
        wclient = EmbedLogClient(e2e_server.ws_url(), origin="watcher")
        watcher = Watcher(wconfig, wclient)

        results: list[int] = []
        t = threading.Thread(target=lambda: results.append(
            watcher.run(timeout=5.0)), daemon=True)
        t.start()
        time.sleep(0.5)

        ic = EmbedLogClient(e2e_server.ws_url(), origin="e2e")
        ic.inject_log("PYTEST", "e2e-watch-evidence matched!")
        ic.close()

        t.join(timeout=6)
        watcher.close()
        assert results[0] >= 1

        lines = evidence.read_text().strip().split("\n")
        assert len(lines) >= 1
        ev = json.loads(lines[0])
        assert ev["watch"] == "detect"
        assert ev["source_id"] == "PYTEST"

    def test_watcher_marker_in_markers_json_and_broadcast(
            self, e2e_server: E2eServer):
        """Watcher with marker:true creates a marker visible in markers.json.
        Also verify a frontend-compatible markers_update event is broadcast
        by connecting a raw WebSocket listener before the watcher runs."""
        import websocket
        from embed_log_sdk import EmbedLogClient
        from embed_log_sdk.watcher import Watcher, WatcherConfig, WatchRule

        # Open a raw WebSocket to capture the broadcast.
        # The control endpoint does not send anything on connect; the
        # server's broadcast loop polls both client commands and the
        # broadcast channel simultaneously, so markers_update will be
        # forwarded when the watcher creates a marker.
        broadcast_ws = websocket.WebSocket()
        broadcast_ws.connect(e2e_server.ws_url(), timeout=10)
        broadcast_ws.settimeout(8.0)

        wconfig = WatcherConfig(
            server_url=e2e_server.ws_url(),
            rules=[WatchRule(name="fatal", sources=["PYTEST"],
                             pattern="FATAL ERROR", marker=True)])
        wclient = EmbedLogClient(e2e_server.ws_url(), origin="watcher")
        watcher = Watcher(wconfig, wclient)

        results: list[int] = []
        t = threading.Thread(target=lambda: results.append(
            watcher.run(timeout=5.0)), daemon=True)
        t.start()
        time.sleep(0.5)

        ic = EmbedLogClient(e2e_server.ws_url(), origin="e2e")
        ic.inject_log("PYTEST", "FATAL ERROR: something broke")
        ic.close()
        t.join(timeout=6)
        watcher.close()
        assert results[0] >= 1

        # Verify markers.json
        log_dirs = sorted(e2e_server.log_dir.iterdir())
        assert log_dirs
        markers_path = log_dirs[0] / "markers.json"
        assert markers_path.exists()
        raw = json.loads(markers_path.read_text())
        markers = raw.get("markers", [])
        assert len(markers) >= 1
        marker = markers[0]
        assert marker["paneId"] == "PYTEST"
        assert marker["description"] == "fatal"

        # Verify the markers_update broadcast was emitted
        broadcast_ws.settimeout(5.0)
        broadcast_found = False
        deadline = time.time() + 5.0
        while time.time() < deadline:
            try:
                frame = broadcast_ws.recv()
            except Exception:
                break
            if not frame:
                break
            parsed = json.loads(frame)
            if parsed.get("type") == "markers_update":
                assert len(parsed["markers"]) >= 1
                assert parsed["markers"][0]["paneId"] == "PYTEST"
                assert parsed["markers"][0]["description"] == "fatal"
                broadcast_found = True
                break
        broadcast_ws.close()
        assert broadcast_found, "markers_update broadcast was not received"

    def test_watcher_marker_list_and_show_via_cli(
            self, e2e_server: E2eServer, embed_log_binary: Path):
        """CLI sessions marker list/show verify watcher-created markers."""
        from embed_log_sdk import EmbedLogClient
        from embed_log_sdk.watcher import Watcher, WatcherConfig, WatchRule

        wconfig = WatcherConfig(
            server_url=e2e_server.ws_url(),
            rules=[WatchRule(name="cli-test", sources=["PYTEST"],
                             pattern="CLI MARKER", marker=True)])
        wclient = EmbedLogClient(e2e_server.ws_url(), origin="watcher")
        watcher = Watcher(wconfig, wclient)

        results: list[int] = []
        t = threading.Thread(target=lambda: results.append(
            watcher.run(timeout=5.0)), daemon=True)
        t.start()
        time.sleep(0.5)

        ic = EmbedLogClient(e2e_server.ws_url(), origin="e2e")
        ic.inject_log("PYTEST", "CLI MARKER test")
        ic.close()
        t.join(timeout=6)
        watcher.close()
        assert results[0] >= 1

        session_id = sorted(e2e_server.log_dir.iterdir())[0].name

        # ---- sessions marker list --json ----
        list_result = subprocess.run(
            [str(embed_log_binary), "sessions", "marker", "list", session_id,
             "--log-dir", str(e2e_server.log_dir), "--json"],
            capture_output=True, text=True, timeout=10,
        )
        assert list_result.returncode == 0, f"list failed: {list_result.stderr}"
        data = json.loads(list_result.stdout)
        markers = data.get("markers", [])
        # Search for our marker by description rather than assuming index 0
        our_markers = [m for m in markers
                       if m.get("description") == "cli-test"
                       and m.get("paneId") == "PYTEST"]
        assert len(our_markers) >= 1, \
            f"marker cli-test not found in {markers}"
        marker = our_markers[0]
        assert marker["paneId"] == "PYTEST"
        assert marker["lineIdx"] >= 0

        # ---- sessions marker show N --json ----
        # Find the original 1-based index from the list output
        # (the --json output includes an "index" field when filtered)
        list_all_result = subprocess.run(
            [str(embed_log_binary), "sessions", "marker", "list", session_id,
             "--log-dir", str(e2e_server.log_dir), "--json"],
            capture_output=True, text=True, timeout=10,
        )
        all_data = json.loads(list_all_result.stdout)
        all_markers = all_data.get("markers", [])
        for i, m in enumerate(all_markers):
            if m.get("description") == "cli-test" and m.get("paneId") == "PYTEST":
                cli_index = i + 1  # 1-based
                break
        else:
            pytest.fail("could not find marker index")

        show_result = subprocess.run(
            [str(embed_log_binary), "sessions", "marker", "show", session_id,
             str(cli_index),
             "--log-dir", str(e2e_server.log_dir), "--json"],
            capture_output=True, text=True, timeout=10,
        )
        assert show_result.returncode == 0, f"show failed: {show_result.stderr}"
        shown = json.loads(show_result.stdout)
        assert shown.get("paneId") == "PYTEST"
        assert shown.get("description") == "cli-test"
        assert shown.get("lineIdx") is not None
