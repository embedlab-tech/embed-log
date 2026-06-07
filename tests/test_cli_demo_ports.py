import os
import socket
import sys
import tempfile
import time
import argparse
import unittest
from pathlib import Path

from backend.cli.demo import DemoRunner, _port_in_use


class DemoPortCleanupTests(unittest.TestCase):
    def test_udp_port_in_use_matches_demo_bind_address(self):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as blocker:
            blocker.bind(("127.0.0.1", 0))
            port = blocker.getsockname()[1]
            self.assertTrue(_port_in_use(port, "udp"))

    @unittest.skipIf(os.name == "nt", "process groups are POSIX-specific")
    def test_cleanup_terminates_child_process_group(self):
        args = argparse.Namespace()
        runner = DemoRunner(args)

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            pid_file = tmp_path / "grandchild.pid"
            parent_script = tmp_path / "parent.py"
            parent_script.write_text(
                "import subprocess, sys, time\n"
                f"p = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n"
                f"open({str(pid_file)!r}, 'w').write(str(p.pid))\n"
                "time.sleep(30)\n",
                encoding="utf-8",
            )

            proc = runner._popen([sys.executable, str(parent_script)])
            runner._processes.append((proc, None))
            deadline = time.monotonic() + 5
            while not pid_file.exists() and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertTrue(pid_file.exists(), "grandchild pid was not written")
            grandchild_pid = int(pid_file.read_text(encoding="utf-8"))

            runner._cleanup()

            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                try:
                    os.kill(grandchild_pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
            else:
                self.fail("demo cleanup left a process-group descendant running")


if __name__ == "__main__":
    unittest.main()
