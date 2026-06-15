#!/usr/bin/env python3
"""Example: run the embed-log watcher.

Listens for log entries matching configured patterns and writes
evidence to a JSONL file, optionally creating UI markers.

Usage:
    python examples/watcher_run.py [--timeout 30] [--config watcher.yml]
"""

from __future__ import annotations

import argparse
import sys

from embed_log_sdk import EmbedLogClient
from embed_log_sdk.watcher import Watcher, WatcherConfig


def main() -> None:
    parser = argparse.ArgumentParser(description="embed-log watcher")
    parser.add_argument("--config", default="watcher.yml", help="watcher config file")
    parser.add_argument("--timeout", type=float, default=None, help="run duration in seconds")
    parser.add_argument("--server", default=None, help="override server URL")
    args = parser.parse_args()

    config = WatcherConfig.from_file(args.config)
    if args.server:
        config.server_url = args.server

    client = EmbedLogClient(config.server_url)
    watcher = Watcher(config, client)

    print(f"Watching {len(config.rules)} rule(s) for up to {args.timeout or 'infinite'}s...")
    try:
        count = watcher.run(timeout=args.timeout)
        print(f"Done. {count} match(es) found.")
    except KeyboardInterrupt:
        count = watcher.run(timeout=0.1)  # drain remaining
        print(f"\nInterrupted. {count} match(es) found.")
    finally:
        watcher.close()
        client.close()


if __name__ == "__main__":
    main()
