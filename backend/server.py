"""Compatibility entrypoint.

`backend.server:main` remains stable for packaged CLI entrypoints.
Runtime implementation lives in `backend.core.runtime`.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Optional

# Backward-compatible re-exports for imports that previously used backend.server
try:
    from .core.models import LogEntry
    from .core.runtime import LogServer, SourceManager, _slug
except Exception:
    repo_root = Path(__file__).resolve().parents[1]
    if str(repo_root) not in sys.path:
        sys.path.insert(0, str(repo_root))
    from backend.core.models import LogEntry
    from backend.core.runtime import LogServer, SourceManager, _slug


def main(argv: Optional[list[str]] = None) -> int:
    try:
        from .cli import main as cli_main
    except Exception:
        repo_root = Path(__file__).resolve().parents[1]
        if str(repo_root) not in sys.path:
            sys.path.insert(0, str(repo_root))
        from backend.cli import main as cli_main
    return cli_main(argv)


if __name__ == "__main__":
    raise SystemExit(main())
