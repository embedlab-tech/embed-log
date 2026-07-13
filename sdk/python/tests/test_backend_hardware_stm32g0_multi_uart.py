"""STM32G0/FT4232H hardware integration test.

The rig exposes four UART sources to embed-log: the ST-LINK control shell and
three independently generated data streams.  This test drives the shell via
embed-log's control API, subscribes to each generator stream, and forwards the
observed records back to embed-log through a loopback UDP source.  It therefore
covers serial routing, writable UART TX, session persistence, and UDP ingest in
one hardware run.

Set ``EMBED_LOG_STM32G0_HARDWARE=1`` to opt in.  The test is intentionally
skipped on normal development and non-lab runners.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import signal
import socket
import subprocess
import time
from pathlib import Path
from typing import Optional

import pytest
import yaml

HARDWARE_GATE = "EMBED_LOG_STM32G0_HARDWARE"
ARTIFACT_DIR_ENV = "EMBED_LOG_STM32G0_ARTIFACT_DIR"
CONTROL_PORT_ENV = "EMBED_LOG_STM32G0_CONTROL_PORT"

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
COUNTER_RE = {
    source: re.compile(rf"^\[{source}\] INFO Counter=(\d+)$")
    for source in UART_PROFILES
}
FORWARDED_RE = re.compile(r"^FORWARDED (USART[134]): \[(USART[134])\] INFO Counter=(\d+)$")
SAMPLE_COUNT = 500
CAPTURE_TIMEOUT_SECONDS = 75


class HardwareServer:
    """Manages an embed-log process and preserves its output for artifacts."""

    def __init__(self, binary: Path, config_path: Path, frontend: Path, ws_port: int, output: Path):
        self.binary = binary
        self.config_path = config_path
        self.frontend = frontend
        self.ws_port = ws_port
        self.output = output
        self.process: Optional[subprocess.Popen[str]] = None
        self.stdout: list[str] = []

    def start(self) -> None:
        self.process = subprocess.Popen(
            [
                str(self.binary),
                "run",
                "--config",
                str(self.config_path),
                "--frontend-dir",
                str(self.frontend),
                "--no-open-browser",
                "--host",
                "127.0.0.1",
                "--ws-port",
                str(self.ws_port),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                self._write_output()
                raise RuntimeError(
                    f"embed-log exited early (code {self.process.returncode}):\n"
                    + "".join(self.stdout)
                )
            line = self.process.stdout.readline()  # type: ignore[union-attr]
            if line:
                self.stdout.append(line)
                if "UI ready at" in line:
                    return
            time.sleep(0.05)
        self.stop()
        raise RuntimeError("embed-log did not start within 20 seconds")

    def stop(self) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGINT)
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self._write_output()
        self.process = None

    def _write_output(self) -> None:
        self.output.write_text("".join(self.stdout), encoding="utf-8")


def free_port(sock_type: int) -> int:
    with socket.socket(socket.AF_INET, sock_type) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def run_cli_json(binary: Path, *args: str) -> dict:
    result = subprocess.run([str(binary), *args], check=True, capture_output=True, text=True)
    return json.loads(result.stdout)


def saved_messages(log_path: Path) -> list[str]:
    timestamp = re.compile(r"^\[[^\]]+\]\s?(.*)$")
    return [
        timestamp.sub(r"\1", line)
        for line in log_path.read_text(encoding="utf-8", errors="replace").splitlines()
        if line.strip()
    ]


def assert_contiguous(counters: list[int], source: str) -> None:
    assert len(counters) >= SAMPLE_COUNT, f"{source} produced only {len(counters)} counters"
    assert counters == list(range(counters[0], counters[0] + len(counters))), (
        f"{source} counters are not contiguous: {counters[:10]} ... {counters[-10:]}"
    )


def shell_write(client: object, command: str) -> None:
    """Send one shell command at a time; Zephyr's shell RX buffer is small."""
    client.tx_write("CONTROL", command + "\n")  # type: ignore[attr-defined]
    time.sleep(0.35)


@pytest.fixture(scope="session")
def embed_log_binary() -> Path:
    installed = shutil.which("embed-log")
    if installed:
        return Path(installed)
    repo_root = Path(__file__).resolve().parents[3]
    for candidate in (repo_root / "target" / "release" / "embed-log", repo_root / "target" / "debug" / "embed-log"):
        if candidate.exists():
            return candidate
    pytest.skip("embed-log binary not found in PATH or target/")


@pytest.fixture(scope="session")
def frontend_dir() -> Path:
    frontend = Path(__file__).resolve().parents[3] / "frontend"
    if not frontend.exists():
        pytest.skip("frontend directory not found")
    return frontend


@pytest.fixture(scope="session")
def stm32g0_ports() -> dict[str, str]:
    if os.environ.get(HARDWARE_GATE) != "1":
        pytest.skip(f"set {HARDWARE_GATE}=1 to run the STM32G0 hardware test")
    ports = dict(DEFAULT_PORTS)
    ports["CONTROL"] = os.environ.get(CONTROL_PORT_ENV, ports["CONTROL"])
    for source in UART_PROFILES:
        ports[source] = os.environ.get(f"EMBED_LOG_STM32G0_{source}_PORT", ports[source])
    missing = [f"{source}={path}" for source, path in ports.items() if not Path(path).exists()]
    if missing:
        pytest.skip("STM32G0 UART paths are unavailable: " + ", ".join(missing))
    return ports


@pytest.fixture
def artifact_dir(tmp_path: Path) -> Path:
    configured = os.environ.get(ARTIFACT_DIR_ENV)
    path = Path(configured) if configured else tmp_path / "stm32g0-artifacts"
    path.mkdir(parents=True, exist_ok=True)
    return path


def test_stm32g0_four_uart_sources_and_udp_forwarding(
    embed_log_binary: Path,
    frontend_dir: Path,
    stm32g0_ports: dict[str, str],
    artifact_dir: Path,
) -> None:
    """Persist isolated generator streams and their Python-forwarded UDP copies."""
    from embed_log_sdk import EmbedLogClient

    ws_port = free_port(socket.SOCK_STREAM)
    udp_port = free_port(socket.SOCK_DGRAM)
    logs_dir = artifact_dir / "logs"
    config_path = artifact_dir / "embed-log-stm32g0.yml"
    config = {
        "version": 1,
        "server": {
            "host": "127.0.0.1",
            "ws_port": ws_port,
            "app_name": "STM32G0 four-UART hardware integration",
            "timestamp_mode": "absolute",
        },
        "logs": {"dir": str(logs_dir)},
        "baudrate": 115200,
        "sources": [
            {
                "name": source,
                "label": source,
                "type": "uart",
                "port": path,
                "baudrate": UART_PROFILES[source]["baudrate"] if source in UART_PROFILES else 115200,
            }
            for source, path in stm32g0_ports.items()
        ]
        + [{"name": "FORWARDED_UDP", "label": "Python forwarded UDP", "type": "udp", "port": udp_port}],
        "tabs": [
            {"label": "Control", "panes": ["CONTROL"]},
            {"label": "Generators A", "panes": ["USART1", "USART3"]},
            {"label": "Generators B", "panes": ["USART4"]},
            {"label": "Forwarded", "panes": ["FORWARDED_UDP"]},
        ],
    }
    config_path.write_text(yaml.safe_dump(config, sort_keys=False), encoding="utf-8")
    server = HardwareServer(embed_log_binary, config_path, frontend_dir, ws_port, artifact_dir / "embed-log.stdout.log")
    server.start()

    observed: dict[str, list[int]] = {source: [] for source in UART_PROFILES}
    try:
        # Give the loopback UDP task a moment to bind before forwarding records.
        time.sleep(0.5)
        with EmbedLogClient.from_config(config_path, origin="stm32g0-hardware") as client, socket.socket(
            socket.AF_INET, socket.SOCK_DGRAM
        ) as forward_socket:
            client.subscribe(list(UART_PROFILES))
            try:
                shell_write(client, "scenario stop")
                for profile in UART_PROFILES.values():
                    shell_write(client, f"uart {profile['peripheral']} baud {profile['baudrate']}")
                for profile in UART_PROFILES.values():
                    shell_write(client, f"gen {profile['peripheral']} interval {profile['interval_ms']}")
                    shell_write(client, f"gen {profile['peripheral']} random off")
                    shell_write(client, f"gen {profile['peripheral']} start")

                deadline = time.monotonic() + CAPTURE_TIMEOUT_SECONDS
                while time.monotonic() < deadline and any(len(values) < SAMPLE_COUNT for values in observed.values()):
                    for entry in client.entries(timeout=0.5):
                        pattern = COUNTER_RE.get(entry.source_id)
                        match = pattern.fullmatch(entry.message) if pattern else None
                        if match is None:
                            continue
                        observed[entry.source_id].append(int(match.group(1)))
                        forward_socket.sendto(
                            f"FORWARDED {entry.source_id}: {entry.message}".encode(),
                            ("127.0.0.1", udp_port),
                        )
                        if all(len(values) >= SAMPLE_COUNT for values in observed.values()):
                            break

                for source, counters in observed.items():
                    assert_contiguous(counters, source)
            finally:
                shell_write(client, "scenario stop")
                for profile in UART_PROFILES.values():
                    if profile["baudrate"] != 115200:
                        shell_write(client, f"uart {profile['peripheral']} baud 115200")
    finally:
        server.stop()

    sessions = run_cli_json(embed_log_binary, "sessions", "list", "--dir", str(logs_dir), "--json")["sessions"]
    assert len(sessions) == 1, f"expected one session, got {sessions}"
    manifest = run_cli_json(
        embed_log_binary, "sessions", "info", sessions[0]["id"], "--dir", str(logs_dir), "--json"
    )
    assert manifest["html_status"] == "ready"
    assert Path(manifest["session_html"]).exists()

    source_files = {source: Path(path) for source, path in manifest["source_files"].items()}
    expected_sources = {"CONTROL", *UART_PROFILES, "FORWARDED_UDP"}
    assert set(source_files) == expected_sources
    for source, path in source_files.items():
        assert path.exists(), f"missing {source} log: {path}"

    for source, pattern in COUNTER_RE.items():
        messages = saved_messages(source_files[source])
        counters = [int(match.group(1)) for message in messages if (match := pattern.fullmatch(message))]
        assert_contiguous(counters, source)
        for other in set(UART_PROFILES) - {source}:
            assert not any(f"[{other}]" in message for message in messages), (
                f"{source} contains traffic from {other}: {messages}"
            )

    control_messages = saved_messages(source_files["CONTROL"])
    assert any("scenario stop" in message for message in control_messages)
    forwarded = [
        match.groups() for message in saved_messages(source_files["FORWARDED_UDP"])
        if (match := FORWARDED_RE.fullmatch(message))
    ]
    for source in UART_PROFILES:
        forwarded_counters = [int(counter) for forwarded_source, label, counter in forwarded if forwarded_source == label == source]
        assert_contiguous(forwarded_counters, f"forwarded {source}")
