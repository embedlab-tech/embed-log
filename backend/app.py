from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import Optional

from .core.naming import slugify
from .sources import LogSource, UartSource, UdpSource

DEFAULT_WS_UI = str((Path(__file__).resolve().parents[1] / "frontend" / "index.html").resolve())


def parse_source(name: str, spec: str, default_baudrate: int) -> LogSource:
    if ":" not in spec:
        raise ValueError(
            f"--source {name!r}: invalid spec {spec!r}. Use uart:/dev/path[@baud] or udp:PORT"
        )

    kind, arg = spec.split(":", 1)
    kind = kind.lower().strip()
    arg = arg.strip()

    if kind == "uart":
        if "@" in arg:
            path, baud = arg.rsplit("@", 1)
            try:
                return UartSource(path, int(baud))
            except ValueError:
                raise ValueError(
                    f"--source {name!r}: uart baudrate must be integer, got {baud!r}"
                )
        return UartSource(arg, default_baudrate)

    if kind == "udp":
        try:
            return UdpSource(int(arg))
        except ValueError:
            raise ValueError(
                f"--source {name!r}: udp port must be an integer, got {arg!r}"
            )

    raise ValueError(
        f"--source {name!r}: invalid spec {spec!r}. Use uart:/dev/path[@baud] or udp:PORT"
    )


def run_app(
    *,
    source_names: list[str],
    source_objects: dict[str, LogSource],
    inject_ports: dict[str, int],
    forward_ports: dict[str, list[int]],
    tabs: list[dict],
    logs_root: Path,
    host: str,
    verbose: bool,
    ws_port: int,
    ws_ui: str,
    config_path: Optional[str],
    job_id: Optional[str],
    open_browser: bool,
    app_name: str,
    default_light_theme: Optional[str],
    default_dark_theme: Optional[str],
    queue_maxsize: int = 20000,
) -> int:
    tab_label_by_source: dict[str, str] = {}
    for tab in tabs:
        for pane in tab["panes"]:
            tab_label_by_source[pane] = tab["label"]

    base_session_id = datetime.now().astimezone().strftime("%Y-%m-%d_%H-%M-%S")
    if job_id:
        base_session_id = f"{base_session_id}__{slugify(job_id)}"

    session_id = base_session_id
    session_dir = logs_root / session_id
    i = 1
    while session_dir.exists():
        session_id = f"{base_session_id}_{i}"
        session_dir = logs_root / session_id
        i += 1
    session_dir.mkdir(parents=True, exist_ok=True)

    sources = []
    for name in source_names:
        tab_label = tab_label_by_source.get(name, "session")
        log_name = f"{slugify(tab_label)}__{slugify(name)}__{session_id}.log"
        sources.append({
            "name": name,
            "source": source_objects[name],
            "inject_port": inject_ports.get(name),
            "forward_ports": forward_ports.get(name, []),
            "log_file": str(session_dir / log_name),
        })

    from .core import LogServer

    LogServer(
        sources,
        tabs,
        session_id=session_id,
        session_dir=str(session_dir),
        logs_root=str(logs_root),
        host=host,
        verbose=verbose,
        ws_port=ws_port,
        ws_ui=ws_ui,
        config_path=config_path,
        job_id=job_id,
        open_browser=open_browser,
        app_name=app_name,
        theme_defaults={
            "light": default_light_theme,
            "dark": default_dark_theme,
        },
        queue_maxsize=queue_maxsize,
    ).run_forever()
    return 0
