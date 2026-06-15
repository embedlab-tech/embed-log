#!/usr/bin/env python3
"""Example: send a UART TX command via the embed-log SDK."""

from embed_log_sdk import EmbedLogClient

with EmbedLogClient.from_config("embed-log.yml", origin="pytest") as client:
    bytes_written = client.tx_write("DUT_UART", "version\r\n")
    print(f"Wrote {bytes_written} bytes to DUT_UART.")
