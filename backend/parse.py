from __future__ import annotations

import argparse
import json
import re
from datetime import datetime
from pathlib import Path


SCRIPT_TABS_RE = re.compile(r"window\.TABS\s*=\s*(\[.*?\])\s*;", re.S)
LOGDATA_MARKER_RE = re.compile(r"var\s+_logData\s*=\s*", re.S)


def _safe_name(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", name).strip("_") or "source"


def _extract_json_at(text: str, start: int) -> object | None:
    i = start
    while i < len(text) and text[i].isspace():
        i += 1
    if i >= len(text) or text[i] not in "[{":
        return None

    opener = text[i]
    closer = "}" if opener == "{" else "]"
    depth = 0
    in_str = False
    esc = False

    for j in range(i, len(text)):
        ch = text[j]
        if in_str:
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == '"':
                in_str = False
            continue
        if ch == '"':
            in_str = True
        elif ch == opener:
            depth += 1
        elif ch == closer:
            depth -= 1
            if depth == 0:
                return json.loads(text[i:j + 1])
    return None


def _extract_json_after_marker(text: str, marker_re: re.Pattern) -> object:
    for m in marker_re.finditer(text):
        try:
            obj = _extract_json_at(text, m.end())
        except Exception:
            obj = None
        if obj is not None:
            return obj
    raise SystemExit("could not find embedded log data in HTML")


def _normalize_ts(value: str) -> str:
    value = value.strip()
    if re.match(r"\d{4}-\d{2}-\d{2}T", value):
        return value
    m = re.match(r"(\d{2})-(\d{2})\s+(\d{2}:\d{2}:\d{2}(?:\.\d+)?)", value)
    if m:
        year = datetime.now().astimezone().year
        return f"{year}-{m.group(1)}-{m.group(2)}T{m.group(3)}"
    return value


def _extract_tabs(text: str) -> list[dict]:
    m = SCRIPT_TABS_RE.search(text)
    if not m:
        return []
    try:
        return json.loads(m.group(1))
    except Exception:
        return []


def run_parse(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="embed-log parse",
        description="Parse an exported embed-log session.html back into raw session log files.",
    )
    parser.add_argument("html", help="embed-log session.html file")
    parser.add_argument("--output", "-o", help="output session directory; default: parsed_<html-name>_<time>")
    args = parser.parse_args(argv)

    html_path = Path(args.html)
    if not html_path.is_file():
        raise SystemExit(f"HTML file not found: {html_path}")

    text = html_path.read_text(encoding="utf-8", errors="replace")
    tabs = _extract_tabs(text)
    log_data = _extract_json_after_marker(text, LOGDATA_MARKER_RE)
    if not isinstance(log_data, dict):
        raise SystemExit("embedded log data has unexpected format")

    stamp = datetime.now().astimezone().strftime("%Y-%m-%d_%H-%M-%S")
    out_dir = Path(args.output) if args.output else Path(f"parsed_{html_path.stem}_{stamp}")
    out_dir.mkdir(parents=True, exist_ok=True)

    source_files: dict[str, str] = {}
    total_lines = 0
    first_ts = None

    for source, entries in log_data.items():
        if not isinstance(entries, list):
            continue
        source_name = str(source)
        out_file = out_dir / f"{_safe_name(source_name)}.log"
        lines = []
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            ts = _normalize_ts(str(entry.get("ts") or ""))
            msg = str(entry.get("text") or "")
            if not ts:
                continue
            if first_ts is None:
                first_ts = ts
            lines.append(f"[{ts}] {msg}\n")
        out_file.write_text("".join(lines), encoding="utf-8")
        source_files[source_name] = str(out_file)
        total_lines += len(lines)

    manifest = {
        "session_id": out_dir.name,
        "session_dir": str(out_dir),
        "started_at": first_ts,
        "config_path": None,
        "source": "parsed_html",
        "source_html": str(html_path),
        "tabs": tabs,
        "source_files": source_files,
        "session_html": str(html_path),
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    print(f"Parsed:  {html_path}")
    print(f"Output:  {out_dir}")
    print(f"Sources: {len(source_files)}")
    print(f"Lines:   {total_lines}")
    print("Next:")
    print(f"  embed-log slice {out_dir} --last 10m")
    return 0
