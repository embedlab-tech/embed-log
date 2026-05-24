import tempfile
import threading
import unittest
from pathlib import Path

from unittest.mock import patch

from backend.core.runtime import LogServer
from backend.sources.uart import UartSource


class _DummyManager:
    def __init__(self):
        self.calls = []

    def wait_until_flushed(self):
        self.calls.append(("wait", None))

    def flush_log_file(self, *, locked=False):
        self.calls.append(("flush", locked))


class _DummySession:
    def __init__(self, html_path: Path):
        self.html_path = html_path
        self.manifest_calls = []

    def write_manifest(self, **kwargs):
        self.manifest_calls.append(kwargs)


class _DummyExporter:
    def __init__(self, result=True):
        self.result = result
        self.reasons = []

    def export_html(self, reason: str) -> bool:
        self.reasons.append(reason)
        return self.result


class RuntimeExportTests(unittest.TestCase):
    def _make_server(self):
        with tempfile.TemporaryDirectory() as td:
            html_path = Path(td) / "session.html"
            server = LogServer.__new__(LogServer)
            server._export_lock = threading.Lock()
            server._rotate_lock = threading.Lock()
            server._managers = [_DummyManager(), _DummyManager()]
            server._session = _DummySession(html_path)
            server._exporter = _DummyExporter(True)
            server._session_info = {
                "html_status": "pending",
                "html_ready": False,
                "html_updated_at": None,
                "html_error": None,
            }
            server._publish_html_state = lambda: None
            return server

    def test_export_session_html_flushes_managers(self):
        server = self._make_server()

        ok = LogServer.export_session_html(server, "manual_ui")

        self.assertTrue(ok)
        for mgr in server._managers:
            self.assertEqual(mgr.calls, [("wait", None), ("flush", False)])

    def test_export_session_html_locked_flush(self):
        server = self._make_server()

        ok = LogServer.export_session_html(server, "rotate:manual_ui", log_files_locked=True)

        self.assertTrue(ok)
        for mgr in server._managers:
            self.assertEqual(mgr.calls, [("wait", None), ("flush", True)])


class _FakeSerial:
    def __init__(self, chunks, stop_event):
        self._chunks = list(chunks)
        self._stop = stop_event
        self.is_open = True

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.is_open = False
        return False

    def read(self, _size):
        if self._chunks:
            return self._chunks.pop(0)
        self._stop.set()
        return b""

    def write(self, _data):
        return None


class UartSourceTests(unittest.TestCase):
    def test_split_utf8_character_is_decoded_after_line_reassembly(self):
        stop = threading.Event()
        lines = []
        src = UartSource("loop://")
        fake_serial = _FakeSerial([b"prefix \xe2\x82", b"\xac suffix\n"], stop)

        with patch("serial.serial_for_url", return_value=fake_serial):
            src._run(lines.append, stop, "UART")

        self.assertEqual(lines, ["prefix € suffix"])


if __name__ == "__main__":
    unittest.main()
