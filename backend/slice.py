from __future__ import annotations

import argparse
import json
import re
import shutil
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timedelta
from pathlib import Path
from typing import Optional

from .config import ConfigError, load_config

ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")
TS_RE = re.compile(r"\[(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)\]")


@dataclass
class LogLine:
    source: str
    path: Path
    ts: datetime
    text: str


def _strip_ansi(s: str) -> str:
    return ANSI_RE.sub("", s)


def _format_ts(ts: datetime, mode: str) -> str:
    if mode == "full":
        return ts.isoformat(timespec="milliseconds")
    return ts.strftime("%H:%M:%S.%f")[:-3]


def _compact_line(line: LogLine, *, include_source: bool, time_format: str) -> str:
    text = _strip_ansi(line.text).rstrip("\n")
    # Remove full ISO timestamps already represented by the compact timestamp.
    text = TS_RE.sub("", text).lstrip()
    # Drop redundant source tags from per-line payload; the file name or
    # combined.log prefix already identifies the source.
    text = text.replace(f"[{line.source}]", "")
    source_variants = {line.source, line.source.replace("-", "_"), line.source.replace("_", "-")}
    for variant in source_variants:
        text = re.sub(r"\[" + re.escape(variant) + r"\]", "", text, flags=re.I)
    text = re.sub(r"\s+", " ", text).strip()
    parts = [f"[{_format_ts(line.ts, time_format)}]"]
    if include_source:
        parts.append(f"[{line.source}]")
    if text:
        parts.append(text)
    return " ".join(parts)


def _message_only(line: LogLine) -> str:
    text = _compact_line(line, include_source=False, time_format="time")
    return re.sub(r"^\[[^\]]+\]\s*", "", text).strip()


def _severity(line: LogLine) -> str:
    msg = _message_only(line).lower()
    if re.search(r"<\s*(err|error)\s*>", msg):
        return "error"
    if re.search(r"<\s*(wrn|warn|warning)\s*>", msg):
        return "warning"
    if re.search(r"<\s*(dbg|debug)\s*>", msg):
        return "debug"
    if re.search(r"<\s*(inf|info)\s*>", msg):
        return "info"
    if re.search(r"\berror\b|\berr\b|hardfault|\bfault\b|panic|assert(?:ion)? failed", msg):
        return "error"
    if re.search(r"\bwarn(?:ing)?\b|\bwrn\b", msg):
        return "warning"
    if re.search(r"\bdebug\b|\bdbg\b", msg):
        return "debug"
    if re.search(r"\binfo\b|\binf\b", msg):
        return "info"
    return "other"


def _event_kind(line: LogLine) -> Optional[str]:
    msg = _message_only(line).lower()
    if _severity(line) in {"error", "warning"}:
        return _severity(line)
    if re.search(r"reboot|restart|reset|boot sequence|bootloader|watchdog|brownout|power on", msg):
        return "transition"
    if re.search(r"drop(?:ped)?|lost|overflow|overrun|queue full|near capacity", msg):
        return "dropped"
    if re.search(r"\btx\b|\[tx", msg):
        return "tx"
    return None


def _normalize_msg(line: LogLine) -> str:
    msg = _message_only(line).lower()
    msg = re.sub(r"0x[0-9a-f]+", "0x*", msg)
    msg = re.sub(r"\b\d+(?:\.\d+)?\b", "*", msg)
    msg = re.sub(r"\s+", " ", msg).strip()
    return msg


def _parse_ts(text: str) -> Optional[datetime]:
    m = TS_RE.search(_strip_ansi(text))
    if not m:
        return None
    raw = m.group(1).replace("Z", "+00:00")
    try:
        return datetime.fromisoformat(raw)
    except ValueError:
        return None


def _parse_duration(value: str) -> timedelta:
    m = re.fullmatch(r"\s*(\d+(?:\.\d+)?)\s*([smhd]?)\s*", value, re.I)
    if not m:
        raise argparse.ArgumentTypeError("duration must look like 30s, 10m, 2h, or 1d")
    n = float(m.group(1))
    unit = (m.group(2) or "s").lower()
    if unit == "s":
        return timedelta(seconds=n)
    if unit == "m":
        return timedelta(minutes=n)
    if unit == "h":
        return timedelta(hours=n)
    if unit == "d":
        return timedelta(days=n)
    raise argparse.ArgumentTypeError(f"unsupported duration unit: {unit}")


def _parse_datetime(value: str) -> datetime:
    raw = value.strip().replace("Z", "+00:00")
    try:
        return datetime.fromisoformat(raw)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(
            "timestamp must be ISO-like, e.g. 2026-05-14T12:15:30+02:00"
        ) from exc


def _resolve_logs_root(args) -> Path:
    if args.logs_dir:
        return Path(args.logs_dir)
    if args.config and Path(args.config).is_file():
        try:
            cfg = load_config(args.config)
            return Path(cfg.get("log_dir", "logs/"))
        except ConfigError as exc:
            raise SystemExit(f"Config error: {exc}")
    return Path("logs/")


def _latest_session(logs_root: Path) -> Path:
    if not logs_root.is_dir():
        raise SystemExit(f"logs directory not found: {logs_root}")
    sessions = [p for p in logs_root.iterdir() if p.is_dir() and (p / "manifest.json").is_file()]
    if not sessions:
        raise SystemExit(f"no sessions found in: {logs_root}")
    return max(sessions, key=lambda p: (p.stat().st_mtime, p.name))


def _load_manifest(session_dir: Path) -> dict:
    path = session_dir / "manifest.json"
    if not path.is_file():
        raise SystemExit(f"manifest not found: {path}")
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise SystemExit(f"failed to read manifest {path}: {exc}")


def _source_files(session_dir: Path, manifest: dict, selected: Optional[list[str]]) -> dict[str, Path]:
    raw = manifest.get("source_files") or {}
    files: dict[str, Path] = {}
    for source, value in raw.items():
        if selected and source not in selected:
            continue
        p = Path(value)
        if not p.is_absolute() and not p.is_file():
            p = session_dir / p.name
        files[source] = p
    if selected:
        missing = sorted(set(selected) - set(files))
        if missing:
            raise SystemExit(f"source(s) not found in session: {', '.join(missing)}")
    if not files:
        raise SystemExit("no source log files found")
    return files


def _read_lines(files: dict[str, Path]) -> list[LogLine]:
    out: list[LogLine] = []
    for source, path in files.items():
        if not path.is_file():
            continue
        with path.open("r", encoding="utf-8", errors="replace") as f:
            for text in f:
                ts = _parse_ts(text)
                if ts is not None:
                    out.append(LogLine(source=source, path=path, ts=ts, text=text.rstrip("\n")))
    return out


def _default_output_dir(session_dir: Path) -> Path:
    stamp = datetime.now().astimezone().strftime("slice_%Y-%m-%d_%H-%M-%S")
    return session_dir / "slices" / stamp


def _with_default_tz(ts: Optional[datetime], tz) -> Optional[datetime]:
    if ts is not None and ts.tzinfo is None:
        return ts.replace(tzinfo=tz)
    return ts


def _windows_from_args(args, lines: list[LogLine]) -> list[tuple[datetime, datetime]]:
    if not lines:
        raise SystemExit("no timestamped log lines found")
    default_tz = lines[0].ts.tzinfo

    if args.last:
        end = max(line.ts for line in lines)
        return [(end - args.last, end)]

    if args.around:
        before = args.before or timedelta(minutes=5)
        after = args.after or timedelta(minutes=5)
        around = _with_default_tz(args.around, default_tz)
        return [(around - before, around + after)]

    if args.grep:
        before = args.before or timedelta(minutes=2)
        after = args.after or timedelta(minutes=2)
        rx = re.compile(args.grep, re.I if args.ignore_case else 0)
        matches = [line.ts for line in lines if rx.search(_strip_ansi(line.text))]
        if not matches:
            raise SystemExit(f"no matches for grep: {args.grep}")
        return [(ts - before, ts + after) for ts in matches]

    start = _with_default_tz(args.from_ts, default_tz) or min(line.ts for line in lines)
    end = _with_default_tz(args.to_ts, default_tz) or max(line.ts for line in lines)
    return [(start, end)]


def _in_windows(ts: datetime, windows: list[tuple[datetime, datetime]]) -> bool:
    return any(start <= ts <= end for start, end in windows)


def _write_analysis(out_dir: Path, lines: list[LogLine], *, grep: Optional[str], ignore_case: bool, before: Optional[timedelta], after: Optional[timedelta], time_format: str) -> None:
    analysis = out_dir / "analysis"
    analysis.mkdir(parents=True, exist_ok=True)

    # severity index
    sev_items = [{"ts": l.ts.isoformat(), "time": _format_ts(l.ts, time_format), "source": l.source, "severity": _severity(l), "message": _message_only(l)} for l in lines]
    sev_counts = Counter(i["severity"] for i in sev_items)
    important = [i for i in sev_items if i["severity"] in {"error", "warning"}]
    (analysis / "severity.json").write_text(json.dumps({"counts": dict(sev_counts), "items": important}, indent=2), encoding="utf-8")
    md = ["# Severity index", "", "## Counts", ""] + [f"- {k}: {v}" for k, v in sorted(sev_counts.items())]
    md += ["", "## Errors / warnings", ""] + [f"- `{i['time']}` [{i['source']}] {i['severity']}: {i['message']}" for i in important[:500]]
    (analysis / "severity.md").write_text("\n".join(md) + "\n", encoding="utf-8")

    # repetition clusters
    groups: dict[str, list[LogLine]] = defaultdict(list)
    for l in lines:
        groups[_normalize_msg(l)].append(l)
    clusters = []
    for key, vals in groups.items():
        if len(vals) < 2:
            continue
        vals = sorted(vals, key=lambda x: x.ts)
        clusters.append({"pattern": key, "count": len(vals), "first": vals[0].ts.isoformat(), "last": vals[-1].ts.isoformat(), "sources": sorted({v.source for v in vals}), "example": _message_only(vals[0])})
    clusters.sort(key=lambda c: c["count"], reverse=True)
    (analysis / "repetition.json").write_text(json.dumps({"clusters": clusters}, indent=2), encoding="utf-8")
    md = ["# Repetition clusters", ""] + [f"- x{c['count']} `{c['first']}` → `{c['last']}` sources={','.join(c['sources'])}: {c['example']}" for c in clusters[:100]]
    (analysis / "repetition.md").write_text("\n".join(md) + "\n", encoding="utf-8")

    # derived timeline
    timeline = []
    for l in lines:
        kind = _event_kind(l)
        if kind:
            timeline.append({"ts": l.ts.isoformat(), "time": _format_ts(l.ts, time_format), "source": l.source, "kind": kind, "severity": _severity(l), "message": _message_only(l)})
    (analysis / "timeline.json").write_text(json.dumps({"events": timeline}, indent=2), encoding="utf-8")
    md = ["# Derived timeline", ""] + [f"- `{e['time']}` [{e['source']}] {e['kind']}: {e['message']}" for e in timeline[:1000]]
    (analysis / "timeline.md").write_text("\n".join(md) + "\n", encoding="utf-8")

    # anchors for --grep
    if grep:
        rx = re.compile(grep, re.I if ignore_case else 0)
        b = before or timedelta(minutes=2)
        a = after or timedelta(minutes=2)
        anchors = []
        for idx, l in enumerate(lines):
            if not rx.search(_strip_ansi(l.text)):
                continue
            ctx = [x for x in lines if l.ts - b <= x.ts <= l.ts + a]
            anchors.append({"anchor": {"ts": l.ts.isoformat(), "source": l.source, "message": _message_only(l)}, "context": [{"ts": x.ts.isoformat(), "source": x.source, "message": _message_only(x)} for x in ctx]})
        (analysis / "anchors.json").write_text(json.dumps({"grep": grep, "anchors": anchors}, indent=2), encoding="utf-8")
        md = [f"# Anchors: `{grep}`", ""]
        for n, item in enumerate(anchors, 1):
            anc = item["anchor"]
            md += [f"## Anchor {n}: `{anc['ts']}` [{anc['source']}] {anc['message']}", ""]
            md += [f"- `{c['ts']}` [{c['source']}] {c['message']}" for c in item["context"]]
            md += [""]
        (analysis / "anchors.md").write_text("\n".join(md), encoding="utf-8")

    # incident windows: simple clustering of problematic events
    problem = [l for l in lines if _event_kind(l) in {"error", "warning", "dropped", "transition"}]
    incidents = []
    if problem:
        cur = [problem[0]]
        for l in problem[1:]:
            if l.ts - cur[-1].ts <= timedelta(minutes=2):
                cur.append(l)
            else:
                incidents.append(cur); cur = [l]
        incidents.append(cur)
    incident_objs = []
    for grp in incidents:
        if len(grp) < 2 and _severity(grp[0]) not in {"error", "warning"}:
            continue
        kinds = Counter(_event_kind(x) or "other" for x in grp)
        incident_objs.append({"start": grp[0].ts.isoformat(), "end": grp[-1].ts.isoformat(), "sources": sorted({x.source for x in grp}), "signals": dict(kinds), "examples": [_message_only(x) for x in grp[:10]]})
    (analysis / "incidents.json").write_text(json.dumps({"incidents": incident_objs}, indent=2), encoding="utf-8")
    md = ["# Incident summary", ""]
    for n, inc in enumerate(incident_objs, 1):
        md += [f"## Incident {n}: `{inc['start']}` → `{inc['end']}`", f"- sources: {', '.join(inc['sources'])}", f"- signals: {inc['signals']}", "- examples:"]
        md += [f"  - {x}" for x in inc["examples"]] + [""]
    (analysis / "incidents.md").write_text("\n".join(md) + "\n", encoding="utf-8")


def _write_outputs(out_dir: Path, session_dir: Path, manifest: dict, files: dict[str, Path], lines: list[LogLine], windows: list[tuple[datetime, datetime]], *, raw: bool, time_format: str, grep: Optional[str], ignore_case: bool, before: Optional[timedelta], after: Optional[timedelta]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    selected = [line for line in lines if _in_windows(line.ts, windows)]
    by_source = {source: [] for source in files}
    for line in selected:
        by_source.setdefault(line.source, []).append(line)

    for source, source_lines in by_source.items():
        safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", source).strip("_") or "source"
        if raw:
            content = "".join(line.text + "\n" for line in source_lines)
        else:
            content = "".join(_compact_line(line, include_source=False, time_format=time_format) + "\n" for line in source_lines)
        (out_dir / f"{safe}.log").write_text(content, encoding="utf-8")

    selected_sorted = sorted(selected, key=lambda line: line.ts)
    if raw:
        combined = "".join(f"[{line.source}] {line.text}\n" for line in selected_sorted)
    else:
        combined = "".join(_compact_line(line, include_source=True, time_format=time_format) + "\n" for line in selected_sorted)
    (out_dir / "combined.log").write_text(combined, encoding="utf-8")

    shutil.copy2(session_dir / "manifest.json", out_dir / "session_manifest.json")
    slice_manifest = {
        "session_id": manifest.get("session_id") or session_dir.name,
        "session_dir": str(session_dir),
        "windows": [[a.isoformat(), b.isoformat()] for a, b in windows],
        "sources": sorted(files),
        "line_count": len(selected),
        "line_count_by_source": {k: len(v) for k, v in by_source.items()},
        "format": "raw" if raw else "compact",
        "time_format": time_format,
    }
    (out_dir / "manifest.json").write_text(json.dumps(slice_manifest, indent=2), encoding="utf-8")

    summary = [
        "# embed-log slice",
        "",
        f"- session: `{slice_manifest['session_id']}`",
        f"- session dir: `{session_dir}`",
        f"- output dir: `{out_dir}`",
        f"- total lines: {len(selected)}",
        "- windows:",
    ]
    summary += [f"  - `{a.isoformat()}` → `{b.isoformat()}`" for a, b in windows]
    summary += ["", "## Sources"]
    summary += [f"- `{src}`: {len(by_source.get(src, []))} lines → `{src}.log`" for src in sorted(files)]
    summary += ["", "## Combined", "", "- `combined.log` — all selected lines sorted by timestamp"]
    summary += ["", "## Analysis", "", "- `analysis/severity.md` — counts and error/warning index", "- `analysis/repetition.md` — repeated message clusters", "- `analysis/timeline.md` — derived important-event timeline", "- `analysis/incidents.md` — heuristic incident windows", "- `analysis/anchors.md` — grep neighborhoods, only when `--grep` is used"]
    (out_dir / "summary.md").write_text("\n".join(summary) + "\n", encoding="utf-8")
    _write_analysis(out_dir, selected_sorted, grep=grep, ignore_case=ignore_case, before=before, after=after, time_format=time_format)


def run_slice(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="embed-log slice",
        description="Extract timestamp-based slices from embed-log session logs.",
    )
    parser.add_argument("session_dir", nargs="?", help="session directory; defaults to the most recent session")
    parser.add_argument("--config", "-c", default="embed-log.yml", help="config path used to find logs.dir (default: embed-log.yml)")
    parser.add_argument("--logs-dir", help="override logs directory")
    parser.add_argument("--output", "-o", help="output directory; default: <session>/slices/slice_<time>")
    parser.add_argument("--source", action="append", help="source/pane name to include; repeatable")

    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--last", type=_parse_duration, help="last duration from latest log timestamp, e.g. 10m")
    mode.add_argument("--around", type=_parse_datetime, help="timestamp to extract around")
    mode.add_argument("--grep", help="extract time windows around matching lines")
    parser.add_argument("--from", dest="from_ts", type=_parse_datetime, help="range start timestamp")
    parser.add_argument("--to", dest="to_ts", type=_parse_datetime, help="range end timestamp")
    parser.add_argument("--before", type=_parse_duration, help="context before --around/--grep, e.g. 2m")
    parser.add_argument("--after", type=_parse_duration, help="context after --around/--grep, e.g. 5m")
    parser.add_argument("--ignore-case", action="store_true", help="case-insensitive --grep")
    parser.add_argument("--raw", action="store_true", help="preserve original log lines instead of compact output")
    parser.add_argument("--time-format", choices=["time", "full"], default="time", help="timestamp format for compact output (default: time)")
    args = parser.parse_args(argv)

    session_dir = Path(args.session_dir) if args.session_dir else _latest_session(_resolve_logs_root(args))
    if not session_dir.is_dir():
        raise SystemExit(f"session directory not found: {session_dir}")

    manifest = _load_manifest(session_dir)
    files = _source_files(session_dir, manifest, args.source)
    lines = _read_lines(files)
    windows = _windows_from_args(args, lines)
    out_dir = Path(args.output) if args.output else _default_output_dir(session_dir)
    _write_outputs(out_dir, session_dir, manifest, files, lines, windows, raw=args.raw, time_format=args.time_format, grep=args.grep, ignore_case=args.ignore_case, before=args.before, after=args.after)

    print(f"Session: {session_dir}")
    print(f"Output:  {out_dir}")
    print(f"Lines:   {sum(1 for line in lines if _in_windows(line.ts, windows))}")
    return 0
