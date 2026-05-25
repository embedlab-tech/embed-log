import socket
import tempfile
import time
import unittest
from pathlib import Path

from backend.file_tail_udp import FileUdpForwarder, parse_udp_target


class FileUdpForwarderTests(unittest.TestCase):
    def setUp(self):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.settimeout(0.05)
        self.host, self.port = self.sock.getsockname()

    def tearDown(self):
        self.sock.close()

    def _recv_messages(self, expected: int, timeout: float = 1.0) -> list[str]:
        msgs: list[str] = []
        deadline = time.monotonic() + timeout
        while len(msgs) < expected and time.monotonic() < deadline:
            try:
                data, _ = self.sock.recvfrom(65536)
                msgs.append(data.decode("utf-8", errors="replace"))
            except socket.timeout:
                continue
        return msgs

    def test_parse_udp_target(self):
        self.assertEqual(parse_udp_target("127.0.0.1:6000"), ("127.0.0.1", 6000))
        with self.assertRaises(Exception):
            parse_udp_target("missing-port")
        with self.assertRaises(Exception):
            parse_udp_target("127.0.0.1:not-a-port")

    def test_from_end_only_forwards_new_lines(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "app.log"
            path.write_text("old-one\nold-two\n", encoding="utf-8")
            fwd = FileUdpForwarder(path, self.host, self.port, from_start=False)
            try:
                self.assertEqual(fwd.poll_once(), 0)
                self.assertEqual(self._recv_messages(1, timeout=0.2), [])

                with path.open("a", encoding="utf-8") as fh:
                    fh.write("new-one\nnew-two\n")
                sent = fwd.poll_once()
                self.assertEqual(sent, 2)
                self.assertEqual(self._recv_messages(2), ["new-one", "new-two"])
            finally:
                fwd.close()

    def test_from_start_forwards_existing_lines(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "app.log"
            path.write_text("first\nsecond\n", encoding="utf-8")
            fwd = FileUdpForwarder(path, self.host, self.port, from_start=True)
            try:
                sent = fwd.poll_once()
                self.assertEqual(sent, 2)
                self.assertEqual(self._recv_messages(2), ["first", "second"])
            finally:
                fwd.close()

    def test_truncate_or_replace_reopens_from_start(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "app.log"
            path.write_text("start\n", encoding="utf-8")
            fwd = FileUdpForwarder(path, self.host, self.port, from_start=False)
            try:
                fwd.poll_once()  # open at EOF
                self.assertEqual(self._recv_messages(1, timeout=0.2), [])

                with path.open("a", encoding="utf-8") as fh:
                    fh.write("next\n")
                self.assertEqual(fwd.poll_once(), 1)
                self.assertEqual(self._recv_messages(1), ["next"])

                # Replace/truncate the file and ensure the new content is sent from start.
                path.write_text("x\n", encoding="utf-8")
                self.assertEqual(fwd.poll_once(), 1)
                self.assertEqual(self._recv_messages(1), ["x"])
            finally:
                fwd.close()


if __name__ == "__main__":
    unittest.main()
