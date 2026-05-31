from __future__ import annotations

import queue
import threading

from .models import LogEntry, QueueStats


class TrackedQueue:
    """Bounded queue with throughput and saturation tracking.

    Blocks on put() when full (backpressure — never drops entries).
    Tracks enqueue/dequeue counts, peak depth, and near-full events.
    """

    __slots__ = (
        "_queue", "_maxsize", "_near_full_pct",
        "_enqueued", "_dequeued", "_peak_depth", "_near_full_count",
        "_lock",
    )

    NEAR_FULL_PCT = 0.80

    def __init__(self, maxsize: int = 0):
        if maxsize <= 0:
            maxsize = 0  # 0 = unbounded in Queue
        self._queue: queue.Queue[LogEntry | None] = queue.Queue(maxsize)
        self._maxsize = maxsize
        self._near_full_pct = self.NEAR_FULL_PCT
        self._enqueued = 0
        self._dequeued = 0
        self._peak_depth = 0
        self._near_full_count = 0
        self._lock = threading.Lock()

    @property
    def maxsize(self) -> int:
        return self._maxsize

    def put(self, item: LogEntry | None) -> None:
        self._queue.put(item)
        with self._lock:
            self._enqueued += 1
            depth = self._queue.qsize()
            if depth > self._peak_depth:
                self._peak_depth = depth
            if self._maxsize > 0 and depth >= self._maxsize * self._near_full_pct:
                self._near_full_count += 1

    def get(self) -> LogEntry | None:
        item = self._queue.get()
        with self._lock:
            self._dequeued += 1
        return item

    def task_done(self) -> None:
        self._queue.task_done()

    def join(self) -> None:
        self._queue.join()

    def stats(self) -> QueueStats:
        with self._lock:
            qsize = self._queue.qsize()
            return QueueStats(
                maxsize=self._maxsize,
                depth=qsize,
                utilization_pct=round(qsize / self._maxsize * 100, 1) if self._maxsize > 0 else 0.0,
                enqueued=self._enqueued,
                dequeued=self._dequeued,
                peak_depth=self._peak_depth,
                near_full_events=self._near_full_count,
            )

    def clear_stats(self) -> None:
        with self._lock:
            self._enqueued = 0
            self._dequeued = 0
            self._peak_depth = self._queue.qsize()
            self._near_full_count = 0
