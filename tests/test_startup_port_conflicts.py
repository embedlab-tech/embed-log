import socket
import tempfile
import threading
import unittest
from pathlib import Path

from backend.core.runtime import LogServer, SourceManager
from backend.sources import LogSource, UdpSource


def free_udp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind(("0.0.0.0", 0))
        return sock.getsockname()[1]


def free_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class IdleSource(LogSource):
    def start(self, on_line, stop: threading.Event, name: str) -> None:
        # Deliberately no background work; used to exercise inject/forward binds.
        return


class StartupPortConflictTests(unittest.TestCase):
    def test_udp_bind_failure_is_reported_during_start(self):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as blocker:
            blocker.bind(("0.0.0.0", 0))
            port = blocker.getsockname()[1]

            with tempfile.TemporaryDirectory() as tmp:
                mgr = SourceManager(
                    name="UDP_A",
                    source=UdpSource(port),
                    log_file=str(Path(tmp) / "udp.log"),
                    socket_host="127.0.0.1",
                )
                try:
                    with self.assertRaisesRegex(RuntimeError, r"UDP_A.*UdpSource"):
                        mgr.start()
                finally:
                    mgr.stop()

    def test_inject_bind_failure_is_reported_during_start(self):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as blocker:
            blocker.bind(("127.0.0.1", 0))
            blocker.listen(1)
            port = blocker.getsockname()[1]

            with tempfile.TemporaryDirectory() as tmp:
                mgr = SourceManager(
                    name="SRC_A",
                    source=IdleSource(),
                    log_file=str(Path(tmp) / "src.log"),
                    socket_host="127.0.0.1",
                    inject_port=port,
                )
                try:
                    with self.assertRaisesRegex(RuntimeError, r"SRC_A.*inject TCP.*127\.0\.0\.1"):
                        mgr.start()
                finally:
                    mgr.stop()

    def test_forward_bind_failure_is_reported_during_start(self):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as blocker:
            blocker.bind(("127.0.0.1", 0))
            blocker.listen(1)
            port = blocker.getsockname()[1]

            with tempfile.TemporaryDirectory() as tmp:
                mgr = SourceManager(
                    name="SRC_A",
                    source=IdleSource(),
                    log_file=str(Path(tmp) / "src.log"),
                    socket_host="127.0.0.1",
                    forward_ports=[port],
                )
                try:
                    with self.assertRaisesRegex(RuntimeError, r"SRC_A.*forward TCP.*127\.0\.0\.1"):
                        mgr.start()
                finally:
                    mgr.stop()

    def test_stop_releases_udp_and_inject_ports_immediately(self):
        udp_port = free_udp_port()
        inject_port = free_tcp_port()

        with tempfile.TemporaryDirectory() as tmp:
            mgr = SourceManager(
                name="SRC_A",
                source=UdpSource(udp_port),
                log_file=str(Path(tmp) / "src.log"),
                socket_host="127.0.0.1",
                inject_port=inject_port,
            )
            mgr.start()
            mgr.stop()

            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp_probe:
                udp_probe.bind(("0.0.0.0", udp_port))

            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as tcp_probe:
                tcp_probe.bind(("127.0.0.1", inject_port))

    def test_log_server_start_failure_releases_already_started_ports(self):
        udp_port = free_udp_port()
        inject_port = free_tcp_port()

        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as blocker:
            blocker.bind(("0.0.0.0", 0))
            blocked_udp_port = blocker.getsockname()[1]

            with tempfile.TemporaryDirectory() as tmp:
                tmp_path = Path(tmp)
                session_dir = tmp_path / "session"
                session_dir.mkdir()
                server = LogServer(
                    sources=[
                        {
                            "name": "SRC_A",
                            "source": UdpSource(udp_port),
                            "log_file": str(tmp_path / "a.log"),
                            "inject_port": inject_port,
                        },
                        {
                            "name": "SRC_B",
                            "source": UdpSource(blocked_udp_port),
                            "log_file": str(tmp_path / "b.log"),
                        },
                    ],
                    tabs=[],
                    session_id="session",
                    session_dir=str(session_dir),
                    logs_root=str(tmp_path),
                    host="127.0.0.1",
                )

                with self.assertRaisesRegex(RuntimeError, r"SRC_B.*UdpSource"):
                    server.start()

                with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp_probe:
                    udp_probe.bind(("0.0.0.0", udp_port))

                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as tcp_probe:
                    tcp_probe.bind(("127.0.0.1", inject_port))


if __name__ == "__main__":
    unittest.main()
