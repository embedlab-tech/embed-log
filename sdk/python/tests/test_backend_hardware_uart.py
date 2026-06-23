"""Hardware-backed backend UART log persistence test.

Starts a real embed-log server against a physical UART device, drives the
attached sandbox firmware through the control WebSocket, and verifies that the
backend saved the deterministic serial output to the session log file.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any, Generator, Optional

import pytest
import yaml

UART_PORT_ENV = "EMBED_LOG_HARDWARE_UART_PORT"
UART_BAUD_ENV = "EMBED_LOG_HARDWARE_UART_BAUD"
DEFAULT_UART_PORT = "/dev/ttyACM3"
DEFAULT_UART_BAUD = 115200
EXPECTED_LINE_COUNT = 10

ESP_LOG_RE = re.compile(r"^[IWED] \(\d+\) \S+:\s*(.*)$")
FILE_TS_RE = re.compile(r"^\[[^\]]+\]\s?(.*)$")
SEQ_RE = re.compile(r"^#(\d+)\s")
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


class HardwareServer:
    """Manages a real embed-log server process for hardware tests."""

    def __init__(self, binary: Path, config_path: Path, frontend: Path, host: str, port: int):
        self.binary = binary
        self.config_path = config_path
        self.frontend = frontend
        self.host = host
        self.port = port
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
                self.host,
                "--ws-port",
                str(self.port),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        deadline = time.time() + 20
        while time.time() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(
                    f"server exited early (code {self.process.returncode}):\n" + "".join(self.stdout)
                )
            line = self.process.stdout.readline()  # type: ignore[union-attr]
            if line:
                self.stdout.append(line)
                if "UI ready at" in line:
                    return
            time.sleep(0.05)
        self.stop()
        raise RuntimeError("server did not start within 20s")

    def stop(self) -> None:
        if self.process is None:
            return
        self.process.send_signal(signal.SIGINT)
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.process = None

    def ws_url(self) -> str:
        return f"ws://{self.host}:{self.port}/api/v1/control"


class Lcg:
    """Python port of embed-sandbox's minimal LCG."""

    MASK64 = (1 << 64) - 1
    U32_MAX_F32 = struct.unpack("!f", struct.pack("!f", float(0xFFFFFFFF)))[0]

    def __init__(self, seed: int):
        self.state = (seed + 1) & self.MASK64

    def next_u32(self) -> int:
        self.state = (
            (self.state * 6364136223846793005) + 1442695040888963407
        ) & self.MASK64
        return (self.state >> 32) & 0xFFFFFFFF

    @staticmethod
    def _as_i32(value: int) -> int:
        return value if value < (1 << 31) else value - (1 << 32)

    @staticmethod
    def _f32(value: float) -> float:
        return struct.unpack("!f", struct.pack("!f", float(value)))[0]

    def range(self, lo: int, hi: int) -> int:
        if hi <= lo:
            return lo
        span = hi - lo + 1
        n = self._as_i32(self.next_u32())
        return lo + (n % span)

    def f32(self, lo: float, hi: float) -> float:
        ratio = self._f32(self._f32(float(self.next_u32())) / self.U32_MAX_F32)
        return self._f32(self._f32(lo) + self._f32(ratio * self._f32(hi - lo)))


TOPICS = ["temp", "humidity", "pressure", "light"]
VALID_LEVELS = {"[INFO]", "[WARN]", "[ERROR]", "[DEBUG]"}


def format_expected_log_line(rng: Lcg, n: int) -> str:
    choice = rng.range(0, 14)
    if choice == 0:
        body = f"[INFO]  System temperature: {rng.f32(15.0, 55.0):.1f} °C, pressure: {rng.f32(1000.0, 1030.0):.1f} hPa"
    elif choice == 1:
        body = f"[WARN]  Memory usage at {rng.range(50, 99)} %"
    elif choice == 2:
        body = f"[INFO]  Sensor read cycle #{n} completed in {rng.range(5, 80)} ms"
    elif choice == 3:
        body = f"[ERROR] Communication timeout on I2C bus {rng.range(0, 2)}"
    elif choice == 4:
        body = f"[INFO]  Heart rate: {rng.range(55, 100)} BPM, SpO2: {rng.range(94, 100)} %"
    elif choice == 5:
        body = f"[DEBUG] ADC reading: channel={rng.range(0, 7)}, value={rng.range(0, 4095)}"
    elif choice == 6:
        body = f"[WARN]  Battery voltage: {rng.f32(3.0, 4.2):.2f} V, low threshold: 3.30 V"
    elif choice == 7:
        body = f"[INFO]  NTP sync completed, offset: {rng.range(-10, 10)} ms"
    elif choice == 8:
        body = f"[ERROR] CRC mismatch on flash sector {rng.range(0, 1023)}"
    elif choice == 9:
        body = f"[INFO]  WiFi RSSI: {rng.range(-90, -30)} dBm, channel: {rng.range(1, 13)}"
    elif choice == 10:
        body = f"[INFO]  File system check: {rng.range(4000, 4100)} blocks OK, {rng.range(0, 3)} bad"
    elif choice == 11:
        body = (
            f"[DEBUG] SPI transaction: {rng.range(8, 1024)} bytes, "
            f"cs={rng.range(0, 2)}, speed={rng.range(10, 80)} MHz"
        )
    elif choice == 12:
        body = f"[WARN]  RTC drift: {rng.f32(-5.0, 5.0):.1f} ppm, temperature compensated"
    else:
        body = f"[INFO]  MQTT publish: topic=sensors/{TOPICS[rng.range(0, 3)]}, qos={rng.range(0, 2)}"
    return f"#{n} {body}"


def expected_lines(seed: int, count: int) -> list[str]:
    rng = Lcg(seed)
    return [format_expected_log_line(rng, n) for n in range(count)]


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def run_cli_json(binary: Path, *args: str) -> Any:
    result = subprocess.run(
        [str(binary), *args],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


@pytest.fixture(scope="session")
def embed_log_binary() -> Path:
    installed = shutil.which("embed-log")
    if installed:
        return Path(installed)

    repo_root = Path(__file__).resolve().parent.parent.parent.parent
    candidates = [
        repo_root / "target" / "debug" / "embed-log",
        repo_root / "target" / "release" / "embed-log",
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    pytest.skip("embed-log binary not found in PATH or target/")


@pytest.fixture(scope="session")
def frontend_dir() -> Path:
    repo_root = Path(__file__).resolve().parent.parent.parent.parent
    frontend = repo_root / "frontend"
    if frontend.exists():
        return frontend
    pytest.skip("frontend directory not found")



@pytest.fixture(scope="session")
def hardware_uart() -> tuple[str, int]:
    port = os.environ.get(UART_PORT_ENV, DEFAULT_UART_PORT)
    baud = int(os.environ.get(UART_BAUD_ENV, str(DEFAULT_UART_BAUD)))
    path = Path(port)
    if not path.exists():
        pytest.skip(f"hardware UART port not present: {port}")
    return port, baud


@pytest.fixture
def temp_dir() -> Generator[Path, None, None]:
    with tempfile.TemporaryDirectory(prefix="embed-log-hw-uart-") as directory:
        yield Path(directory)


@pytest.fixture
def hardware_server(
    embed_log_binary: Path,
    frontend_dir: Path,
    hardware_uart: tuple[str, int],
    temp_dir: Path,
) -> Generator[tuple[HardwareServer, Path], None, None]:
    port_name, baud = hardware_uart
    ws_port = free_port()
    logs_dir = temp_dir / "logs"
    config_path = temp_dir / "embed-log-hardware.yml"
    config = {
        "version": 1,
        "server": {
            "host": "127.0.0.1",
            "ws_port": ws_port,
            "app_name": "embed-log backend hardware test",
            "timestamp_mode": "absolute",
        },
        "logs": {"dir": str(logs_dir)},
        "baudrate": baud,
        "sources": [
            {
                "name": "DUT_UART",
                "label": "DUT UART",
                "type": "uart",
                "port": port_name,
                "baudrate": baud,
            }
        ],
        "tabs": [{"label": "Device", "panes": ["DUT_UART"]}],
    }
    config_path.write_text(yaml.safe_dump(config))

    server = HardwareServer(embed_log_binary, config_path, frontend_dir, "127.0.0.1", ws_port)
    server.start()
    try:
        yield server, logs_dir
    finally:
        server.stop()


def strip_saved_prefix(line: str) -> str:
    match = FILE_TS_RE.match(line)
    return match.group(1) if match else line


def strip_ansi(line: str) -> str:
    return ANSI_RE.sub("", line)


def strip_esp_prefix(line: str) -> str:
    match = ESP_LOG_RE.match(line)
    return match.group(1) if match else line


def normalized_messages(log_path: Path) -> list[str]:
    text = log_path.read_text(encoding="utf-8", errors="replace")
    return [
        strip_esp_prefix(strip_ansi(strip_saved_prefix(line)))
        for line in text.splitlines()
        if line.strip()
    ]


def wait_for_log_file(logs_dir: Path, timeout: float = 10.0) -> Path:
    deadline = time.time() + timeout
    while time.time() < deadline:
        log_files = sorted(logs_dir.glob("*/*.log"))
        if log_files:
            assert len(log_files) == 1, f"expected one UART log file, found {log_files}"
            return log_files[0]
        time.sleep(0.1)
    raise AssertionError(f"no log file created under {logs_dir}")




def extract_first_sequence_block(messages: list[str], count: int) -> list[str]:
    for start, message in enumerate(messages):
        if not message.startswith("#0 "):
            continue
        block = messages[start : start + count]
        if len(block) != count:
            continue
        seqs: list[int] = []
        for line in block:
            match = SEQ_RE.match(line)
            if match is None:
                break
            seqs.append(int(match.group(1)))
        if seqs == list(range(count)):
            return block
    raise AssertionError(f"no contiguous #0..#{count - 1} block found in messages: {messages}")


def wait_for_sequence_block(log_path: Path, count: int, timeout: float = 10.0) -> tuple[list[str], list[str]]:
    deadline = time.time() + timeout
    last_messages: list[str] = []
    while time.time() < deadline:
        last_messages = normalized_messages(log_path)
        try:
            block = extract_first_sequence_block(last_messages, count)
            return last_messages, block
        except AssertionError:
            time.sleep(0.1)
    raise AssertionError(
        f"no contiguous #0..#{count - 1} block found in {log_path}: {last_messages}"
    )

def test_uart_backend_logs_are_saved_deterministically(
    embed_log_binary: Path,
    hardware_server: tuple[HardwareServer, Path],
) -> None:
    from embed_log_sdk import EmbedLogClient

    server, logs_dir = hardware_server
    log_path = wait_for_log_file(logs_dir)

    with EmbedLogClient.from_config(server.config_path, origin="hardware-test") as client:
        client.tx_write("DUT_UART", "pause\n")
        client.tx_write("DUT_UART", "seed 42\n")
        client.tx_write("DUT_UART", "rate 20\n")
        client.tx_write("DUT_UART", "resume\n")
        _, sequence_block = wait_for_sequence_block(log_path, EXPECTED_LINE_COUNT, timeout=10.0)
        client.tx_write("DUT_UART", "pause\n")

    server.stop()

    sessions = run_cli_json(
        embed_log_binary,
        "sessions",
        "list",
        "--dir",
        str(logs_dir),
        "--json",
    )["sessions"]
    assert len(sessions) == 1, f"expected one session, got: {sessions}"
    session = sessions[0]
    session_id = session["id"]

    manifest = run_cli_json(
        embed_log_binary,
        "sessions",
        "info",
        session_id,
        "--dir",
        str(logs_dir),
        "--json",
    )
    assert manifest["html_status"] == "ready"
    session_html = Path(manifest["session_html"])
    assert session_html.exists(), f"missing exported html: {session_html}"

    source_files = manifest["source_files"]
    assert set(source_files) == {"DUT_UART"}
    source_log = Path(source_files["DUT_UART"])
    assert source_log == log_path
    assert source_log.exists(), f"missing UART source log: {source_log}"

    messages = normalized_messages(source_log)
    assert "seed set to 42 (counter reset)" in messages
    assert "rate set to 20 ms" in messages
    assert "output resumed" in messages
    assert "output paused" in messages
    assert_valid_levels(sequence_block)
    assert sequence_block == expected_lines(42, EXPECTED_LINE_COUNT)

