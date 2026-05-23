# BENCHMARKING

## What benchmark coverage exists today

The main benchmark is:
- `benchmarks/serial_stress.py`

It stress-tests the backend using virtual UART endpoints (`socket://...`) so the real CLI/runtime path is exercised without physical serial hardware.

## What it is good for

- regression-checking throughput-oriented backend changes
- validating UART ingestion changes
- checking for dropped, duplicated, or corrupt benchmark frames
- comparing backend modes under controlled synthetic traffic

## Quick commands

From repo root:

```bash
# smoke
python benchmarks/serial_stress.py --sources 1 --duration 10 --line-rate 100 --mode disk-only

# stronger baseline
python benchmarks/serial_stress.py --sources 4 --duration 60 --line-rate 1000 --mode disk-only
```

## Important limitations

Current benchmark documentation/review notes indicate it is useful but not yet a perfect source of truth.

Known concerns from existing review notes:
- success criteria may be too tied to process exit code rather than frame-integrity metrics,
- some benchmark modes need verification to ensure they are really implemented as documented,
- benchmark results should be interpreted alongside missing/duplicate/corrupt counters, not just a printed `PASS` label.

## Canonical benchmark-related files

- `benchmarks/serial_stress.py`
- `BENCHMARK.md` — detailed benchmark usage/reference notes

## When to run benchmarks

Run benchmark coverage when changing:
- UART ingestion internals
- runtime write/broadcast hot paths
- buffering/queueing behavior
- anything intended as a backend performance improvement
