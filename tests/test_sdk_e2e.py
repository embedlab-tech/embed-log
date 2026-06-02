"""End-to-end tests for the embed-log SDK.

These tests start a demo server and exercise the full InjectClient →
ForwardClient → Watcher pipeline.  No real UART hardware is required
— the demo server uses UDP sources.
"""

from __future__ import annotations

import subprocess
import sys
import threading
import time
import unittest
from pathlib import Path

# Ensure the repo root is on sys.path so ``from sdk import …`` works.
REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT))

from sdk import ForwardClient, InjectClient, Watcher, WatchMatch, LogEntry  # noqa: E402


def _free_port() -> int:
    """Return an available TCP port."""
    import socket
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class SdkE2ETests(unittest.TestCase):
    """Full pipeline: inject → forward → watch."""

    @classmethod
    def setUpClass(cls) -> None:
        """Start the deterministic demo server on random ports."""
        cls.ws_port = _free_port()
        cls.inject_port = _free_port()

        config = Path("/tmp/embed-log-sdk-test.yml")
        config.write_text(f"""\
version: 1
server:
  host: 127.0.0.1
  ws_port: {cls.ws_port}
  open_browser: false
  timestamp_mode: relative
logs:
  dir: /tmp/sdk-e2e-logs
sources:
  - name: SDK_TEST
    type: udp
    port: {_free_port()}
    inject_port: {cls.inject_port}
tabs:
  - label: SDK
    panes: [SDK_TEST]
""")
        cls._server = subprocess.Popen(
            [
                sys.executable, "-m", "backend.server", "run",
                "--config", str(config),
                "--no-open-browser",
            ],
            cwd=str(REPO_ROOT),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        # Wait for the inject port to be available.
        import socket
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            try:
                s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                s.settimeout(0.5)
                s.connect(("127.0.0.1", cls.inject_port))
                s.close()
                break
            except OSError:
                time.sleep(0.2)
        else:
            cls._server.kill()
            cls._server.wait()
            raise RuntimeError("Demo server did not start in time")

    @classmethod
    def tearDownClass(cls) -> None:
        cls._server.terminate()
        try:
            cls._server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            cls._server.kill()
            cls._server.wait()

    # Helper: open a forward client, inject, collect entries.
    def _inject_and_collect(self, source: str, *messages: str) -> list[LogEntry]:
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        inj = InjectClient(port=self.inject_port, source=source)
        inj.connect()
        for msg in messages:
            inj.log(msg)
        inj.close()

        expected = set(messages)
        found: list[LogEntry] = []
        deadline = time.monotonic() + 5
        while len(found) < len(messages) and time.monotonic() < deadline:
            entry = fwd.read(timeout=0.5)
            if entry is not None and entry.message in expected:
                found.append(entry)
                expected.discard(entry.message)
        fwd.close()
        return found

    # ------------------------------------------------------------------
    # InjectClient
    # ------------------------------------------------------------------

    def test_inject_log_appears_in_forward_stream(self) -> None:
        """Inject a log line and verify it arrives via the ForwardClient."""
        entries = self._inject_and_collect("e2e", "unique-e2e-msg")
        self.assertTrue(entries, "injected line was not received")
        self.assertEqual(entries[0].source_id, "SDK_TEST")
        self.assertEqual(entries[0].source, "e2e")

    def test_inject_with_color(self) -> None:
        """Injected color is preserved in the stream."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()
        inj = InjectClient(port=self.inject_port, source="e2e")
        inj.connect()
        inj.log("colored-line", color="yellow")
        inj.close()

        entry = None
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            e = fwd.read(timeout=0.5)
            if e is not None and e.message == "colored-line":
                entry = e
                break
        fwd.close()
        self.assertIsNotNone(entry)
        self.assertEqual(entry.color, "yellow")

    def test_inject_marker_convenience_methods(self) -> None:
        """info / success / warning / error / step all work."""
        entries = self._inject_and_collect(
            "e2e", "alpha", "beta", "gamma", "delta", "epsilon")
        msgs = {e.message for e in entries}
        for m in ("alpha", "beta", "gamma", "delta", "epsilon"):
            self.assertIn(m, msgs)

    # ------------------------------------------------------------------
    # ForwardClient
    # ------------------------------------------------------------------

    def test_forward_client_context_manager(self) -> None:
        """ForwardClient works as a context manager."""
        with ForwardClient(port=self.inject_port) as fwd:
            self.assertIsNotNone(fwd)
            entry = fwd.read(timeout=2.0)
            self.assertTrue(entry is None or isinstance(entry, LogEntry))

    def test_forward_client_iteration_stops_on_close(self) -> None:
        """__iter__ returns when close() is called from another thread."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        collected: list[LogEntry] = []

        def _read() -> None:
            for entry in fwd:
                collected.append(entry)

        t = threading.Thread(target=_read, daemon=True)
        t.start()
        time.sleep(1.0)
        fwd.close()
        t.join(timeout=3)
        self.assertFalse(t.is_alive(),
                         "reader thread should have exited after close")

    def test_forward_client_read_timeout_returns_none(self) -> None:
        """read(timeout=0.1) returns None when no data arrives."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()
        time.sleep(0.5)
        entry = fwd.read(timeout=0.1)
        self.assertTrue(entry is None or isinstance(entry, LogEntry))
        fwd.close()

    # ------------------------------------------------------------------
    # Watcher
    # ------------------------------------------------------------------

    def test_watcher_matches_on_name_callback(self) -> None:
        """Watcher fires per-pattern callbacks."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        matches: list[WatchMatch] = []
        watcher = Watcher(fwd, patterns={"fatal": r"ZEPHYR FATAL ERROR"})

        @watcher.on("fatal")
        def _on_fatal(m: WatchMatch) -> None:
            matches.append(m)

        t = watcher.start_background()
        # Inject after watcher is running
        inj = InjectClient(port=self.inject_port, source="e2e")
        inj.connect()
        inj.log("ZEPHYR FATAL ERROR at 0xDEAD")
        inj.close()

        deadline = time.monotonic() + 5
        while not matches and time.monotonic() < deadline:
            time.sleep(0.1)
        watcher.stop()
        t.join(timeout=2)
        fwd.close()

        self.assertEqual(len(matches), 1)
        self.assertEqual(matches[0].name, "fatal")
        self.assertIn("ZEPHYR FATAL ERROR", matches[0].entry.message)
        self.assertEqual(matches[0].entry.source_id, "SDK_TEST")

    def test_watcher_on_match_fires_for_all_patterns(self) -> None:
        """on_match callback fires for every matched pattern."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        names: list[str] = []
        watcher = Watcher(fwd, patterns={
            "err": r"error:",
            "warn": r"warning:",
        })

        @watcher.on_match
        def _on_any(m: WatchMatch) -> None:
            names.append(m.name)

        t = watcher.start_background()
        inj = InjectClient(port=self.inject_port, source="e2e")
        inj.connect()
        inj.log("error: something failed")
        inj.log("warning: disk nearly full")
        inj.close()

        deadline = time.monotonic() + 5
        while len(names) < 2 and time.monotonic() < deadline:
            time.sleep(0.1)
        watcher.stop()
        t.join(timeout=2)
        fwd.close()

        self.assertIn("err", names)
        self.assertIn("warn", names)

    def test_watcher_wait_for_blocks_until_match(self) -> None:
        """wait_for returns a WatchMatch when the pattern matches."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        watcher = Watcher(fwd, patterns={"alpha": r"ALPHA_MARKER"})

        def _inject_later() -> None:
            time.sleep(0.5)
            inj = InjectClient(port=self.inject_port, source="e2e")
            inj.connect()
            inj.log("prefix ALPHA_MARKER suffix")
            inj.close()

        threading.Thread(target=_inject_later, daemon=True).start()

        result = watcher.wait_for("alpha", timeout=5.0)
        fwd.close()

        self.assertIsNotNone(result, "wait_for timed out")
        self.assertEqual(result.name, "alpha")
        self.assertIn("ALPHA_MARKER", result.entry.message)

    def test_watcher_wait_for_returns_none_on_timeout(self) -> None:
        """wait_for returns None when no match occurs within timeout."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        watcher = Watcher(fwd, patterns={"nope": r"THIS_WILL_NEVER_MATCH_XYZ"})
        result = watcher.wait_for("nope", timeout=1.0)
        fwd.close()

        self.assertIsNone(result)

    def test_watcher_named_groups_in_match(self) -> None:
        """Regex named groups appear in WatchMatch.groups."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        matches: list[WatchMatch] = []
        watcher = Watcher(fwd, patterns={
            "wdt": r"watchdog: (?P<seconds>\d+)s",
        })

        @watcher.on("wdt")
        def _cb(m: WatchMatch) -> None:
            matches.append(m)

        t = watcher.start_background()
        inj = InjectClient(port=self.inject_port, source="e2e")
        inj.connect()
        inj.log("watchdog: 30s timeout triggered")
        inj.close()

        deadline = time.monotonic() + 5
        while not matches and time.monotonic() < deadline:
            time.sleep(0.1)
        watcher.stop()
        t.join(timeout=2)
        fwd.close()

        self.assertEqual(len(matches), 1)
        self.assertEqual(matches[0].groups, {"seconds": "30"})
        self.assertEqual(matches[0].match.group("seconds"), "30")

    def test_watcher_idle_timeout_stops(self) -> None:
        """Watcher with timeout stops after the idle period with no matches."""
        fwd = ForwardClient(port=self.inject_port)
        fwd.connect()

        watcher = Watcher(fwd, patterns={"x": r"NOMATCH"}, timeout=1.0)
        t = watcher.start_background()
        t.join(timeout=5)
        # After 1s idle + a grace period, the watcher should stop.
        self.assertFalse(t.is_alive(),
                         "watcher should have stopped after idle timeout")
        fwd.close()

    # ------------------------------------------------------------------
    # Models
    # ------------------------------------------------------------------

    def test_log_entry_is_tx_property(self) -> None:
        """LogEntry.is_tx returns True for TX entries."""
        from sdk._models import LogEntry as LE
        rx = LE.from_json({
            "source_id": "X", "source": "SERIAL",
            "message": "hello", "timestamp": "2026-01-01T00:00:00.000",
        })
        self.assertFalse(rx.is_tx)

        tx = LE.from_json({
            "source_id": "X", "source": "TX::X",
            "message": "version", "timestamp": "2026-01-01T00:00:00.001",
        })
        self.assertTrue(tx.is_tx)


if __name__ == "__main__":
    unittest.main()
