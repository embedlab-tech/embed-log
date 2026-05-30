import socket
import tempfile
import threading
import unittest
from pathlib import Path

from backend.core.runtime import SourceManager
from backend.sources import LogSource, UdpSource


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


if __name__ == "__main__":
    unittest.main()
