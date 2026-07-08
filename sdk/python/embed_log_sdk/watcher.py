"""Watcher — a pattern-matching log observer powered by the embed-log SDK.

The watcher connects to a running embed-log server, subscribes to sources,
and matches incoming log lines against regex patterns.  Matches can be
recorded as evidence (JSONL) and optionally create UI markers.

Backend events from static ``.events.yml`` rules are also available via
``client.subscribe(events=True)`` and ``client.events()``. These coexist with
watcher rules: backend events are matched server-side, while watcher rules are
runtime-defined and matched client-side on ``log.entry`` messages.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import yaml

from .client import EmbedLogClient
from .models import LogEntry


@dataclass
class WatchRule:
    """A single watch rule."""

    name: str
    sources: list[str]
    pattern: str
    marker: bool = False
    _compiled: Optional[re.Pattern] = None

    def __post_init__(self):
        try:
            self._compiled = re.compile(self.pattern)
        except re.error as e:
            raise ValueError(f"invalid regex pattern in watch '{self.name}': {e}") from e

    def match(self, entry: LogEntry) -> Optional[dict]:
        """Match a log entry against this rule. Returns match groups or None."""
        if entry.source_id not in self.sources:
            return None
        m = self._compiled.search(entry.message) if self._compiled else None
        if m is None:
            return None
        return {
            "watch": self.name,
            "source_id": entry.source_id,
            "line_idx": entry.line_idx,
            "timestamp_iso": entry.timestamp_iso,
            "origin": entry.origin,
            "message": entry.message,
            "groups": m.groupdict(),
        }


@dataclass
class WatcherConfig:
    """Parsed watcher configuration."""

    server_url: str
    output_path: Optional[Path] = None
    rules: list[WatchRule] = field(default_factory=list)

    @classmethod
    def from_file(cls, path: str | Path) -> "WatcherConfig":
        config_dir = Path(path).resolve().parent
        with open(path) as f:
            raw = yaml.safe_load(f)

        server_url = raw.get("server", {}).get("url", "ws://127.0.0.1:8080/api/v1/control")
        output_path = None
        if "output" in raw and "path" in raw["output"]:
            output_path = config_dir / Path(raw["output"]["path"])

        rules = []
        for w in raw.get("watch", []):
            rules.append(WatchRule(
                name=w.get("name", "unnamed"),
                sources=w.get("sources", []),
                pattern=w.get("pattern", ""),
                marker=w.get("marker", False),
            ))

        return cls(server_url=server_url, output_path=output_path, rules=rules)


class Watcher:
    """Observes log entries and matches them against watch rules."""

    def __init__(self, config: WatcherConfig, client: EmbedLogClient):
        self.config = config
        self.client = client
        self._evidence_file = None
        if config.output_path:
            config.output_path.parent.mkdir(parents=True, exist_ok=True)
            self._evidence_file = open(config.output_path, "a")  # noqa: SIM115

    @classmethod
    def from_config(cls, config_path: str | Path, client: EmbedLogClient) -> "Watcher":
        config = WatcherConfig.from_file(config_path)
        return cls(config, client)

    def run(self, timeout: Optional[float] = None) -> int:
        """Run the watcher, processing log entries until timeout.

        Returns the number of matches found.
        """
        # Subscribe to the union of all watched sources
        all_sources = list(set(s for rule in self.config.rules for s in rule.sources))
        if all_sources:
            self.client.subscribe(all_sources)

        match_count = 0
        for entry in self.client.entries(timeout=timeout):
            for rule in self.config.rules:
                evidence = rule.match(entry)
                if evidence is None:
                    continue
                match_count += 1
                self._write_evidence(evidence)
                if rule.marker:
                    self.client.create_marker(
                        source_id=entry.source_id,
                        line_idx=entry.line_idx,
                        description=evidence["watch"],
                        timestamp_num=None,
                    )
        return match_count

    def _write_evidence(self, evidence: dict) -> None:
        if self._evidence_file:
            self._evidence_file.write(json.dumps(evidence) + "\n")
            self._evidence_file.flush()

    def close(self) -> None:
        if self._evidence_file:
            self._evidence_file.close()
            self._evidence_file = None
