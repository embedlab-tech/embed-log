# BACKEND_BACKLOG

Prioritized backend backlog distilled from existing notes.

## High priority

1. Bounded queues + backpressure/drop counters
   - make pressure visible instead of silently degrading
   - add metrics for dropped lines and queue saturation

2. Per-source UART baudrate support
   - allow different baudrates across UART sources in one config
   - keep global `baudrate` as an optional default, not a hard constraint
   - validate config clearly when a UART source omits both per-source and global baudrate
   - verify mixed-baud deployments work without affecting UDP sources or session metadata

3. Health/stats endpoints
   - `/health`
   - `/stats`
   - useful for CI and unattended runs

4. Session/export contract cleanup
   - finish documenting current session HTML status/event/API behavior
   - keep live UI and static export behavior aligned

5. Time handling correctness
   - define canonical time model clearly
   - keep session APIs/export consistent across platforms/timezones

## Medium priority

6. Optional server-side replay/retention window
   - useful for reconnecting clients and recent-history visibility

7. Configurable default settings contract
   - YAML-driven defaults where backend must provide authoritative values to UI

8. Export naming consistency
   - unify app/session-based naming for artifacts and downloads where backend participates

## Lower priority / future work

9. Authentication for non-local deployments
10. Broader Python/runtime compatibility maintenance
11. Additional operational observability for long-running CI usage

## Performance/benchmark-specific backlog

- tighten benchmark pass/fail semantics around data integrity
- verify all documented benchmark modes are real
- establish repeatable baseline scenarios for regression comparison
