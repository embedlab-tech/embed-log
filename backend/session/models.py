from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class SessionStats:
    alias: str = ""
    lines: int = 0
    size_kb: int = 0
    time_start: str = ""
    time_end: str = ""
    duration_secs: float | None = None
    markers: int = 0


@dataclass
class SnippetEntry:
    file: str
    label: str
    scope: str
    panes: list[str] = field(default_factory=list)
    line_count: int = 0
    saved_at: str = ""
