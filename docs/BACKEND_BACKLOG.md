# BACKEND_BACKLOG

Prioritized backend backlog distilled from existing notes.

## High priority

1. ~~Bounded queues + backpressure/drop counters~~ **Done**
   - `TrackedQueue` in `backend/core/queue.py` with saturation tracking, `QueueStats` dataclass, and `test_queue_stats`.

2. ~~Health/stats endpoints~~ **Done**
   - `/api/health` — simple `{"status": "ok"}`
   - `/api/stats` — per-source queue stats + totals

3. ~~Time handling correctness~~ **Done**
   - `SessionClock` in `backend/core/clock.py` supporting absolute and relative (`T+HH:MM:SS.mmm`) modes.
   - SessionClock origin set on first observe, tested via `test_runtime_timestamp_mode`.

4. Per-source UART baudrate support
   - `SourceConfig.baudrate: int | None` already exists in `backend/config/models.py`.
   - Validate config clearly when a UART source omits both per-source and global baudrate.
   - Verify mixed-baud deployments work without affecting UDP sources or session metadata.

5. Session/export contract cleanup
   - Finish documenting current session HTML status/event/API behavior.
   - Keep live UI and static export behavior aligned.

## Medium priority

6. Optional server-side replay/retention window
   - Useful for reconnecting clients and recent-history visibility.

7. Configurable default settings contract
   - YAML-driven defaults where backend must provide authoritative values to UI.

8. Export naming consistency
   - Unify app/session-based naming for artifacts and downloads where backend participates.

## Lower priority / future work

9. Authentication for non-local deployments
10. Broader Python/runtime compatibility maintenance
11. Additional operational observability for long-running CI usage

## Performance/benchmark-specific backlog

- Tighten benchmark pass/fail semantics around data integrity.
- Verify all documented benchmark modes are real.
- Establish repeatable baseline scenarios for regression comparison.
