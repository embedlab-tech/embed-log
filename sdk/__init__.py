"""Embed-log SDK — Python client library for interacting with a running
embed-log instance.

**Inject** log entries, markers, and TX commands via an inject port.
**Forward** receive the live log stream from an inject port.
**Watch** the stream for regex patterns and fire callbacks on match.

Typical usage::

    from embed_log.sdk import InjectClient, ForwardClient, Watcher

    # Inject log entries
    with InjectClient(port=5001, source="pytest") as inj:
        inj.step("test started")
        inj.log("boot complete", color="green")

    # Watch for patterns in the log stream
    fwd = ForwardClient(port=5001)
    fwd.connect()

    watcher = Watcher(fwd, patterns={"fatal": r"ZEPHYR FATAL ERROR"})

    @watcher.on("fatal")
    def on_fatal(m):
        print(f"HALT: {m.entry.message}")

    watcher.start()  # blocks
"""

from ._forward import ForwardClient
from ._inject import InjectClient
from ._models import LogEntry, WatchMatch
from ._watcher import Watcher

__all__ = [
    "ForwardClient",
    "InjectClient",
    "LogEntry",
    "WatchMatch",
    "Watcher",
]
