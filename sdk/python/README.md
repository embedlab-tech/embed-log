# embed-log SDK

Python SDK for the [embed-log](https://github.com/embedlab-tech/embed-log) live log viewer.

## Installation

```bash
pip install embed-log-sdk
```

Or install from source:

```bash
cd sdk/python
pip install -e .
```

## Quick start

```python
from embed_log_sdk import EmbedLogClient

# Connect from a running config file
with EmbedLogClient.from_config("embed-log.yml", origin="pytest") as client:
    # Inject a log entry
    client.inject_log("DUT_UART", "test_boot: resetting board", color="cyan")

    # Send a UART command
    client.tx_write("DUT_UART", "version\r\n")

    # Subscribe and stream log entries
    client.subscribe(["DUT_UART"])
    for entry in client.entries(timeout=3.0):
        print(f"[{entry.timestamp_iso}] {entry.source_id}: {entry.message}")
```

### Restarts, crashes and crash decoding

```python
with EmbedLogClient.from_config("embed-log.yml", origin="pytest") as client:
    events, counts = client.list_events()           # restarts/crashes so far, per-source counts
    start = events[-1].sequence if events else 0

    client.tx_write("DUT_UART", "sensor start\r\n")
    crash = client.wait_for_event("crash", "DUT_UART", timeout=10)
    if crash:
        report = client.decode_crash(crash.source_id, crash.sequence)  # needs sources.<ID>.elf
        print("\n".join(report.backtrace))

    client.assert_no_crash(since_sequence=start)    # AssertionError with the decoded backtrace
```

## API

See the docstrings in `embed_log_sdk/client.py` for full details. `LogEntry` carries `level` and
`event` (`"boot"` / `"crash"`) when the server detected them.

## Development

```bash
cd sdk/python
pip install -e ".[dev]"
pytest
```
