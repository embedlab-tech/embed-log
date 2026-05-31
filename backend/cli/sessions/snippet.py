"""sessions snippet — manage selection snippets for a session."""
from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

from ..util import read_manifest, read_session_dir


def _run_sessions_snippet(log_dir: Path, args) -> int:
    if not hasattr(args, "snippet_cmd") or not args.snippet_cmd:
        print(
            "error: specify a snippet command: list, show, or delete", file=sys.stderr
        )
        return 1

    sdir = read_session_dir(log_dir, args.session_id)
    if not sdir:
        print(f"Session not found: {args.session_id}", file=sys.stderr)
        return 1

    manifest = read_manifest(sdir)
    snippets = (manifest or {}).get("snippets", [])
    if not snippets:
        print("No snippets found for this session.", file=sys.stderr)
        return 0

    if args.snippet_cmd == "list":
        print(f"Snippets for session {args.session_id}:")
        print()
        for i, s in enumerate(snippets, 1):
            saved = s.get("saved_at", "?")
            label = s.get("label", "?")
            scope = s.get("scope", "?")
            panes = ",".join(s.get("panes", []))
            lines = s.get("line_count", "?")
            print(f"  {i}. {s['file']}")
            print(
                f"       saved: {saved}  scope: {scope}  panes: {panes}  lines: {lines}"
            )
        if args.json:
            print()
            print(json.dumps(snippets, indent=2))
        return 0

    idx = None
    if args.snippet_cmd == "show":
        if args.snippet_id:
            matches = [
                i
                for i, s in enumerate(snippets)
                if s["file"].endswith(args.snippet_id) or args.snippet_id in s["file"]
            ]
            if len(matches) == 0:
                print(f"No snippet matching {args.snippet_id!r}", file=sys.stderr)
                return 1
            if len(matches) > 1:
                print(
                    f"Multiple snippets match {args.snippet_id!r}, use --index to pick:",
                    file=sys.stderr,
                )
                for m in matches:
                    print(f"  {m + 1}. {snippets[m]['file']}", file=sys.stderr)
                return 1
            idx = matches[0]
        elif args.index is not None:
            if args.index < 1 or args.index > len(snippets):
                print(
                    f"Index {args.index} out of range (1-{len(snippets)})",
                    file=sys.stderr,
                )
                return 1
            idx = args.index - 1
        elif args.last:
            idx = len(snippets) - 1
        else:
            idx = len(snippets) - 1

        s = snippets[idx]
        spath = sdir / s["file"]
        if not spath.is_file():
            print(f"Snippet file not found: {spath}", file=sys.stderr)
            return 1
        print(f"# {s['file']}")
        print(f"# scope: {s.get('scope', '?')}  panes: {','.join(s.get('panes', []))}")
        print(f"# saved: {s.get('saved_at', '?')}  lines: {s.get('line_count', '?')}")
        print()
        sys.stdout.write(spath.read_text(encoding="utf-8"))
        return 0

    if args.snippet_cmd == "delete":
        if args.all:
            snippets_dir = sdir / "snippets"
            if snippets_dir.is_dir():
                shutil.rmtree(snippets_dir)
            manifest["snippets"] = []
            mf_path = sdir / "manifest.json"
            mf_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
            print(f"Deleted all {len(snippets)} snippet(s) from {args.session_id}.")
            return 0

        if args.index is not None:
            if args.index < 1 or args.index > len(snippets):
                print(
                    f"Index {args.index} out of range (1-{len(snippets)})",
                    file=sys.stderr,
                )
                return 1
            idx = args.index - 1
            s = snippets[idx]
            spath = sdir / s["file"]
            if spath.is_file():
                spath.unlink()
            del snippets[idx]
            manifest["snippets"] = snippets
            mf_path = sdir / "manifest.json"
            mf_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
            print(f"Deleted snippet {idx + 1}: {s['file']}")
            return 0

        print("error: specify --index N or --all to delete", file=sys.stderr)
        return 1

    print(f"error: unknown snippet command {args.snippet_cmd!r}", file=sys.stderr)
    return 1
