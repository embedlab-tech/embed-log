"""sessions export — export session data (HTML or merged raw log)."""
from __future__ import annotations

import argparse
import datetime as _dt
import json
import sys
from pathlib import Path

from ..util import (
    iter_sessions,
    parse_duration,
    parse_log_timestamp,
    read_manifest,
    read_session_dir,
    session_stats,
)


def _run_sessions_export(log_dir: Path, args: argparse.Namespace) -> int:
    # ── --missing mode: export every session without existing HTML ──
    if args.missing and not args.session_id:
        if args.format != "html":
            print(
                "error: --missing is only supported for --format html", file=sys.stderr
            )
            return 1
        sessions = iter_sessions(log_dir)
        if not sessions:
            print("No sessions found.", file=sys.stderr)
            return 0
        ok = fail = 0
        for s in sessions:
            sid = s.get("session_id")
            if not sid:
                continue
            sdir = log_dir / sid
            html_path = sdir / "session.html"
            if html_path.is_file():
                # Already has an export, skip
                continue
            # Re-invoke the single-session export path
            sub_args = argparse.Namespace(
                session_id=sid,
                output=None,
                format="html",
                after=None,
                before=None,
                first=None,
                last=None,
                panes=None,
                missing=False,
                log_dir=str(log_dir),
                first_log_at=args.first_log_at,
            )
            rc = _run_sessions_export(log_dir, sub_args)
            if rc == 0:
                ok += 1
            else:
                fail += 1
        total = ok + fail
        if fail:
            print(f"Exported {ok}/{total} session(s), {fail} failed.")
        else:
            print(f"Exported {ok} session(s).")
        return 0 if fail == 0 else 1

    # ── Single-session export (existing behavior) ──
    sdir = read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    manifest = read_manifest(sdir)
    if not manifest:
        print(f"No manifest found for session {args.session_id}", file=sys.stderr)
        return 1

    source_files = manifest.get("source_files", {})
    if not source_files:
        print(
            f"No source files in manifest for session {args.session_id}",
            file=sys.stderr,
        )
        return 1

    # ── Raw format: merge and filter log lines ──
    if args.format == "raw":
        # Resolve which panes to include
        panes: list[str] = []
        if args.panes:
            panes = args.panes
        else:
            panes = sorted(source_files.keys())
        # Parse time filters
        after_dt: _dt.datetime | None = None
        before_dt: _dt.datetime | None = None
        now_local = _dt.datetime.now()

        # Resolve --first / --last into after/before using session time range
        time_start = manifest.get("_time_start", "")
        time_end = manifest.get("_time_end", "")
        if (args.first or args.last) and (not time_start or not time_end):
            stats = session_stats(sdir, manifest)
            time_start = stats.time_start
            time_end = stats.time_end
        time_conflicts = []
        if args.after:
            time_conflicts.append("--after")
        if args.before:
            time_conflicts.append("--before")
        if args.first:
            time_conflicts.append("--first")
        if args.last:
            time_conflicts.append("--last")
        if len(time_conflicts) > 2:
            print(
                f"Conflicting time options: {' and '.join(time_conflicts)}",
                file=sys.stderr,
            )
            return 1
        if args.first and args.last:
            print("--first and --last are mutually exclusive", file=sys.stderr)
            return 1

        if args.first:
            secs = parse_duration(args.first)
            if secs is None:
                print(
                    f"Invalid --first value: {args.first!r} (use e.g. 10m, 1h)",
                    file=sys.stderr,
                )
                return 1
            if not time_start:
                print(
                    "Cannot use --first: session has no recorded start time",
                    file=sys.stderr,
                )
                return 1
            try:
                after_dt = _dt.datetime.fromisoformat(time_start.rstrip("Z"))
                before_dt = after_dt + _dt.timedelta(seconds=secs)
            except ValueError:
                print(f"Cannot parse session start time: {time_start}", file=sys.stderr)
                return 1

        elif args.last:
            secs = parse_duration(args.last)
            if secs is None:
                print(
                    f"Invalid --last value: {args.last!r} (use e.g. 30m, 15m)",
                    file=sys.stderr,
                )
                return 1
            if not time_end:
                print(
                    "Cannot use --last: session has no recorded end time",
                    file=sys.stderr,
                )
                return 1
            try:
                before_dt = _dt.datetime.fromisoformat(time_end.rstrip("Z"))
                after_dt = before_dt - _dt.timedelta(seconds=secs)
            except ValueError:
                print(f"Cannot parse session end time: {time_end}", file=sys.stderr)
                return 1

        if args.after:
            secs = parse_duration(args.after)
            if secs is not None:
                after_dt = now_local - _dt.timedelta(seconds=secs)
            else:
                try:
                    after_dt = _dt.datetime.fromisoformat(args.after)
                except ValueError:
                    print(
                        f"Invalid --after value: {args.after!r} (use 5m, 2h, or ISO)",
                        file=sys.stderr,
                    )
                    return 1

        if args.before:
            secs = parse_duration(args.before)
            if secs is not None:
                before_dt = (
                    now_local - _dt.timedelta(seconds=secs)
                    if not args.after
                    else (
                        after_dt + _dt.timedelta(seconds=secs)
                        if after_dt
                        else now_local - _dt.timedelta(seconds=secs)
                    )
                )
            else:
                try:
                    before_dt = _dt.datetime.fromisoformat(args.before)
                except ValueError:
                    print(
                        f"Invalid --before value: {args.before!r} (use 5m, 2h, or ISO)",
                        file=sys.stderr,
                    )
                    return 1

        # Collect and filter lines
        entries: list[dict] = []
        for pane_name in panes:
            fp_str = source_files.get(pane_name)
            if not fp_str:
                print(f"Pane {pane_name!r} not found in session", file=sys.stderr)
                return 1
            fp = Path(fp_str)
            if not fp.is_file():
                print(f"Log file not found: {fp}", file=sys.stderr)
                return 1
            try:
                with fp.open("r", encoding="utf-8") as f:
                    for raw_line in f:
                        stripped = raw_line.strip()
                        if not stripped:
                            continue
                        ts_str = parse_log_timestamp(stripped)
                        if not ts_str:
                            continue
                        ts_dt = _dt.datetime.fromisoformat(ts_str.rstrip("Z"))
                        if after_dt and ts_dt < after_dt:
                            continue
                        if before_dt and ts_dt > before_dt:
                            continue
                        entries.append(
                            {"ts": ts_str, "line": stripped, "pane": pane_name}
                        )
            except OSError as exc:
                print(f"Error reading {fp}: {exc}", file=sys.stderr)
                return 1

        if not entries:
            print("No matching log entries found.", file=sys.stderr)
            return 1

        # Sort by timestamp, then by pane name for stability
        entries.sort(key=lambda e: (e["ts"], e["pane"]))

        output = Path(args.output) if args.output else sdir / "merged.log"
        try:
            output.parent.mkdir(parents=True, exist_ok=True)
            with output.open("w", encoding="utf-8") as f:
                for entry in entries:
                    if len(panes) > 1:
                        f.write(f"[{entry['pane']}] {entry['line']}\n")
                    else:
                        f.write(entry["line"] + "\n")
        except OSError as exc:
            print(f"Error writing {output}: {exc}", file=sys.stderr)
            return 1

        print(f"Exported: {output}  ({len(entries)} lines)")
        return 0

    # ── HTML format (existing behavior) ──
    from ...session import SessionExporter

    tabs = manifest.get("tabs", [])
    source_labels = manifest.get("pane_labels") or {
        entry.get("name"): entry.get("label", entry.get("name"))
        for entry in manifest.get("sources", [])
        if isinstance(entry, dict) and entry.get("name")
    }
    output = Path(args.output) if args.output else sdir / "session.html"

    first_log_at = args.first_log_at if args.first_log_at is not None else manifest.get("first_log_at")
    timestamp_mode = str(manifest.get("timestamp_mode") or "absolute")
    if timestamp_mode == "relative" and not first_log_at:
        print(
            "warning: relative session has no first_log_at metadata; rebuilt HTML will stay relative-only until you re-export with --first-log-at",
            file=sys.stderr,
        )

    exporter = SessionExporter(
        session_html_path=output,
        source_files=source_files,
        tabs=tabs,
        source_labels=source_labels,
        frontend_plugins=manifest.get("frontend_plugins") or {},
        pane_plugins=manifest.get("pane_plugins") or {},
        plugin_scripts=manifest.get("plugin_scripts") or {},
        timestamp_mode=timestamp_mode,
        first_log_at=first_log_at,
    )
    ok = exporter.export_html("sessions_export")
    if not ok:
        print(f"Export failed for session {args.session_id}", file=sys.stderr)
        return 1

    # Update manifest

    manifest["session_html"] = str(output)
    manifest["html_status"] = "ready"
    manifest["html_updated_at"] = _dt.datetime.now().astimezone().isoformat(timespec="seconds")
    if first_log_at is not None:
        manifest["first_log_at"] = first_log_at
    (sdir / "manifest.json").write_text(
        json.dumps(manifest, indent=2), encoding="utf-8"
    )

    print(f"Exported: {output}")
    return 0
