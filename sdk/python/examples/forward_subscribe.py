#!/usr/bin/env python3
"""Example: subscribe to log sources and print entries."""

from embed_log_sdk import EmbedLogClient

with EmbedLogClient.from_config("embed-log.yml", origin="forward") as client:
    client.subscribe(["DUT_UART", "PYTEST"])

    print("Subscribed. Waiting for log entries...")
    for entry in client.entries(timeout=3.0):
        print(f"[{entry.timestamp_iso}] {entry.source_id} <{entry.origin}>: {entry.message}")
    print("Done.")
