#!/usr/bin/env python3
"""Example: inject a log entry into a running embed-log session."""

from embed_log_sdk import EmbedLogClient

with EmbedLogClient.from_config("embed-log.yml", origin="pytest") as client:
    client.inject_log(
        "DUT_UART",
        "test_boot: resetting board",
        color="cyan",
    )
    print("Log entry injected.")
