import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from backend.core.runtime import LogServer
from backend.sources import UdpSource


def _free_udp_port() -> int:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]
    finally:
        sock.close()


class TailFileIntegrationTests(unittest.TestCase):
    def test_tail_file_forwards_into_session_log(self):
        repo_root = Path(__file__).resolve().parents[1]
        udp_port = _free_udp_port()

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            session_dir = root / "session"
            session_dir.mkdir()
            source_file = root / "app.log"
            source_file.write_text("old line\n", encoding="utf-8")
            log_file = session_dir / "app__file_app__session.log"

            server = LogServer(
                sources=[{
                    "name": "FILE_APP",
                    "source": UdpSource(udp_port),
                    "inject_port": None,
                    "forward_ports": [],
                    "log_file": str(log_file),
                }],
                tabs=[{"label": "App", "panes": ["FILE_APP"]}],
                session_id="2026-01-01_00-00-00",
                session_dir=str(session_dir),
                logs_root=str(root),
                host="127.0.0.1",
                verbose=False,
                ws_port=0,
                ws_ui="",
                config_path=None,
                job_id=None,
                open_browser=False,
                app_name="embed-log",
                theme_defaults={},
                queue_maxsize=100,
            )
            server.start()
            proc = None
            try:
                time.sleep(0.15)
                proc = subprocess.Popen(
                    [
                        sys.executable,
                        "-m",
                        "backend.cli",
                        "tail-file",
                        str(source_file),
                        f"127.0.0.1:{udp_port}",
                        "--poll-interval",
                        "0.05",
                    ],
                    cwd=str(repo_root),
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                )
                time.sleep(0.2)

                with source_file.open("a", encoding="utf-8") as fh:
                    fh.write("hello from adapter\n")
                    fh.write("second line\n")

                deadline = time.monotonic() + 5.0
                content = ""
                while time.monotonic() < deadline:
                    for mgr in server._managers:
                        mgr.wait_until_flushed()
                        mgr.flush_log_file()
                    if log_file.exists():
                        content = log_file.read_text(encoding="utf-8")
                        if "hello from adapter" in content and "second line" in content:
                            break
                    time.sleep(0.05)
                else:
                    self.fail(
                        f"timed out waiting for forwarded lines; "
                        f"log contents={content!r}"
                    )

                self.assertIn("hello from adapter", content)
                self.assertIn("second line", content)
                self.assertNotIn("old line", content)
            finally:
                if proc is not None:
                    proc.terminate()
                    try:
                        proc.communicate(timeout=2.0)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.communicate(timeout=2.0)
                server.stop()


if __name__ == "__main__":
    unittest.main()
