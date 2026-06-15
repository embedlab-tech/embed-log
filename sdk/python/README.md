# embed-log SDK

Python SDK for the [embed-log](https://github.com/krezolekcoder/embed-log) live log viewer.

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

## API

See the docstrings in `embed_log_sdk/client.py` for full details.

## Development

```bash
cd sdk/python
pip install -e ".[dev]"
pytest
```
