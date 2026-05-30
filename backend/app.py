from __future__ import annotations

from datetime import datetime
from pathlib import Path

from .core.naming import slugify
from .parsers import create_parser
from .sources import LogSource, ParsedSource, RawUartSource, RawUdpSource, UartSource, UdpSource

DEFAULT_WS_UI = str((Path(__file__).resolve().parents[1] / "frontend" / "index.html").resolve())


def parse_source_config(name: str, spec: str, default_baudrate: int) -> dict:
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
                return {
                    "name": name,
                    "type": "uart",
                    "port": path,
                    "baudrate": int(baud),
                    "parser": {"type": "text"},
                }
            except ValueError:
                raise ValueError(
                    f"--source {name!r}: uart baudrate must be integer, got {baud!r}"
                )
        return {
            "name": name,
            "type": "uart",
            "port": arg,
            "baudrate": default_baudrate,
            "parser": {"type": "text"},
        }

    if kind == "udp":
        try:
            return {
                "name": name,
                "type": "udp",
                "port": int(arg),
                "parser": {"type": "text"},
            }
        except ValueError:
            raise ValueError(
                f"--source {name!r}: udp port must be an integer, got {arg!r}"
            )

    raise ValueError(
        f"--source {name!r}: invalid spec {spec!r}. Use uart:/dev/path[@baud] or udp:PORT"
    )


def build_source(source_config: dict) -> LogSource:
    source_type = source_config["type"]
    parser_config = source_config.get("parser")

    if source_type == "uart" and parser_config == {"type": "text"}:
        return UartSource(source_config["port"], source_config.get("baudrate", 115200))
    if source_type == "udp" and parser_config == {"type": "text"}:
        return UdpSource(source_config["port"])

    if source_type == "uart":
        raw_source = RawUartSource(source_config["port"], source_config.get("baudrate", 115200))
    elif source_type == "udp":
        raw_source = RawUdpSource(source_config["port"])
    else:
        raise ValueError(f"source {source_config.get('name', '?')!r}: unsupported type {source_type!r}")

    return ParsedSource(raw_source, create_parser(parser_config))


def parse_source(name: str, spec: str, default_baudrate: int) -> LogSource:
    return build_source(parse_source_config(name, spec, default_baudrate))


def run_app(
    *,
    source_names: list[str],
    source_objects: dict[str, LogSource],
    inject_ports: dict[str, int],
    source_labels: dict[str, str],
    forward_ports: dict[str, list[int]],
    tabs: list[dict],
    logs_root: Path,
    host: str,
    verbose: bool,
    ws_port: int,
    ws_ui: str,
    config_path: str | None,
    job_id: str | None,
    open_browser: bool,
    app_name: str,
    default_light_theme: str | None,
    default_dark_theme: str | None,
    queue_maxsize: int = 20000,
    timestamp_mode: str = "absolute",
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
            "label": source_labels.get(name, name),
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
        source_labels=source_labels,
        queue_maxsize=queue_maxsize,
        timestamp_mode=timestamp_mode,
    ).run_forever()
    return 0
