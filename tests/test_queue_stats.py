import queue
import threading
import time
import unittest

from backend.core.queue import TrackedQueue


class TrackedQueueTests(unittest.TestCase):
    def test_put_get_basic(self):
        q = TrackedQueue(maxsize=100)
        q.put("a")
        q.put("b")
        self.assertEqual(q.get(), "a")
        self.assertEqual(q.get(), "b")
        q.task_done()
        q.task_done()
        q.join()

    def test_stats_initial(self):
        q = TrackedQueue(maxsize=1000)
        s = q.stats()
        self.assertEqual(s.maxsize, 1000)
        self.assertEqual(s.depth, 0)
        self.assertEqual(s.enqueued, 0)
        self.assertEqual(s.dequeued, 0)
        self.assertEqual(s.peak_depth, 0)
        self.assertEqual(s.near_full_events, 0)
        self.assertEqual(s.utilization_pct, 0.0)

    def test_stats_after_put_get(self):
        q = TrackedQueue(maxsize=100)
        q.put("x")
        q.put("y")
        q.get()
        s = q.stats()
        self.assertEqual(s.enqueued, 2)
        self.assertEqual(s.dequeued, 1)
        self.assertEqual(s.depth, 1)
        self.assertEqual(s.peak_depth, 2)
        q.task_done()
        q.task_done()

    def test_peak_depth_tracks_max(self):
        q = TrackedQueue(maxsize=100)
        q.put(1)
        q.put(2)
        q.put(3)
        self.assertEqual(q.stats().peak_depth, 3)
        q.get()
        q.get()
        self.assertEqual(q.stats().depth, 1)
        # peak_depth is lifetime max, doesn't decrease
        self.assertEqual(q.stats().peak_depth, 3)
        q.task_done()
        q.task_done()
        q.task_done()

    def test_blocks_when_full(self):
        q = TrackedQueue(maxsize=2)
        q.put("a")
        q.put("b")
        # Third put in a thread should block
        blocked = threading.Event()
        put_ok = False

        def putter():
            nonlocal put_ok
            q.put("c")
            put_ok = True
            blocked.set()

        t = threading.Thread(target=putter, daemon=True)
        t.start()
        t.join(timeout=0.5)
        self.assertFalse(put_ok, "put should have blocked on full queue")
        # Drain one entry
        self.assertEqual(q.get(), "a")
        q.task_done()
        time.sleep(0.1)
        self.assertTrue(put_ok, "put should have unblocked after drain")
        self.assertEqual(q.get(), "b")
        self.assertEqual(q.get(), "c")
        q.task_done()
        q.task_done()
        q.join()

    def test_near_full_tracking(self):
        q = TrackedQueue(maxsize=10)
        # Temporarily lower threshold for testing
        old_pct = TrackedQueue.NEAR_FULL_PCT
        TrackedQueue.NEAR_FULL_PCT = 0.5
        try:
            for i in range(8):
                q.put(i)
        finally:
            TrackedQueue.NEAR_FULL_PCT = old_pct
        s = q.stats()
        self.assertGreater(s.near_full_events, 0)
        # Drain
        for _ in range(8):
            q.get()
            q.task_done()
        q.join()

    def test_near_full_zero_maxsize(self):
        """Unbounded queue (maxsize=0) should never trigger near-full."""
        q = TrackedQueue(maxsize=0)
        for i in range(1000):
            q.put(i)
        s = q.stats()
        self.assertEqual(s.maxsize, 0)
        self.assertEqual(s.utilization_pct, 0.0)
        self.assertEqual(s.near_full_events, 0)
        # Drain
        for _ in range(1000):
            q.get()
            q.task_done()
        q.join()

    def test_clear_stats(self):
        q = TrackedQueue(maxsize=10)
        q.put("a")
        q.put("b")
        q.get()
        q.task_done()
        s = q.stats()
        self.assertEqual(s.enqueued, 2)
        q.clear_stats()
        s2 = q.stats()
        self.assertEqual(s2.enqueued, 0)
        self.assertEqual(s2.dequeued, 0)
        # peak_depth resets to current depth
        self.assertEqual(s2.peak_depth, 1)
        q.get()
        q.task_done()
        q.join()

    def test_terminated_by_none(self):
        """Writer loop pattern: put(None) terminates the getter."""
        q = TrackedQueue(maxsize=10)
        q.put("x")
        q.put(None)
        self.assertEqual(q.get(), "x")
        self.assertIsNone(q.get())
        q.task_done()
        q.task_done()
        q.join()


if __name__ == "__main__":
    unittest.main()
