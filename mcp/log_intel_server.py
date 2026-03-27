#!/usr/bin/env python3
"""
POC MCP server: log intelligence tools for embed-log.
"""

from __future__ import annotations

import argparse
import collections
import re
from pathlib import Path

from _mcp_stdio import StdioMcpServer, ToolSpec


SEVERITY_PATTERNS = {
    "err": re.compile(r"(<err>|error|failed|panic|fault|exception)", re.IGNORECASE),
    "wrn": re.compile(r"(<wrn>|warn|timeout|retry|stale)", re.IGNORECASE),
    "dbg": re.compile(r"(<dbg>|debug|trace)", re.IGNORECASE),
    "inf": re.compile(r"(<inf>|info|started|completed|ok)", re.IGNORECASE),
}


def _read_lines(path: Path) -> list[str]:
    if not path.is_file():
        raise FileNotFoundError(f"log file not found: {path}")
    return path.read_text(encoding="utf-8", errors="replace").splitlines()


def _guess_severity(line: str) -> str:
    for sev in ("err", "wrn", "dbg", "inf"):
        if SEVERITY_PATTERNS[sev].search(line):
            return sev
    return "unknown"


def _normalize_message(line: str) -> str:
    # Strip timestamp and collapse numbers/hex tokens for rough clustering.
    msg = re.sub(r"^\[[^\]]+\]\s*", "", line)
    msg = re.sub(r"\b0x[0-9a-fA-F]+\b", "<hex>", msg)
    msg = re.sub(r"\b\d+\b", "<num>", msg)
    msg = re.sub(r"\s+", " ", msg).strip()
    return msg


def _tool_summarize_errors(args: dict) -> dict:
    path = Path(args["path"])
    limit = int(args.get("limit", 30))
    lines = _read_lines(path)
    err_lines = [ln for ln in lines if _guess_severity(ln) == "err"]
    tail = err_lines[-limit:]
    return {
        "path": str(path),
        "error_count": len(err_lines),
        "last_errors": tail,
    }


def _tool_cluster_failures(args: dict) -> dict:
    path = Path(args["path"])
    top_k = int(args.get("top_k", 10))
    lines = _read_lines(path)
    buckets: collections.Counter[str] = collections.Counter()
    samples: dict[str, str] = {}
    for ln in lines:
        if _guess_severity(ln) != "err":
            continue
        key = _normalize_message(ln)
        buckets[key] += 1
        samples.setdefault(key, ln)
    top = buckets.most_common(top_k)
    clusters = [{"count": count, "signature": sig, "sample": samples[sig]} for sig, count in top]
    return {"path": str(path), "clusters": clusters, "total_clusters": len(buckets)}


def _tool_compare_runs(args: dict) -> dict:
    baseline = Path(args["baseline"])
    candidate = Path(args["candidate"])
    base_lines = _read_lines(baseline)
    cand_lines = _read_lines(candidate)

    def _severity_counts(lines: list[str]) -> dict[str, int]:
        cnt: collections.Counter[str] = collections.Counter()
        for ln in lines:
            cnt[_guess_severity(ln)] += 1
        return dict(cnt)

    base_counts = _severity_counts(base_lines)
    cand_counts = _severity_counts(cand_lines)
    all_keys = sorted(set(base_counts) | set(cand_counts))
    delta = {k: cand_counts.get(k, 0) - base_counts.get(k, 0) for k in all_keys}
    regression = delta.get("err", 0) > 0
    return {
        "baseline": str(baseline),
        "candidate": str(candidate),
        "baseline_counts": base_counts,
        "candidate_counts": cand_counts,
        "delta": delta,
        "regression_suspected": regression,
    }


def build_tools() -> dict[str, ToolSpec]:
    return {
        "summarize_errors": ToolSpec(
            description="Summarize error lines from a log file.",
            input_schema={
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"},
                    "limit": {"type": "integer", "default": 30},
                },
            },
            handler=_tool_summarize_errors,
        ),
        "cluster_failures": ToolSpec(
            description="Cluster recurring error signatures from a log file.",
            input_schema={
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"},
                    "top_k": {"type": "integer", "default": 10},
                },
            },
            handler=_tool_cluster_failures,
        ),
        "compare_runs": ToolSpec(
            description="Compare severity distributions between two log files.",
            input_schema={
                "type": "object",
                "required": ["baseline", "candidate"],
                "properties": {
                    "baseline": {"type": "string"},
                    "candidate": {"type": "string"},
                },
            },
            handler=_tool_compare_runs,
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="POC MCP server for embed-log log intelligence")
    parser.parse_args()
    server = StdioMcpServer(
        name="embed-log-log-intel",
        version="0.1.0",
        tools=build_tools(),
    )
    server.serve()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
