from __future__ import annotations

from dataclasses import dataclass
import hashlib
from pathlib import Path


_FRONTEND_DIR = Path(__file__).resolve().parents[1] / "frontend"
_BUILTIN_PLUGIN_FILES: dict[str, Path] = {
    "hex-coap": _FRONTEND_DIR / "plugin-hex-coap.js",
}


@dataclass(frozen=True)
class ResolvedFrontendPlugin:
    name: str
    kind: str
    sha256: str
    script: str
    builtin: str | None = None
    path_label: str | None = None

    def public_metadata(self) -> dict[str, str]:
        data = {
            "kind": self.kind,
            "sha256": self.sha256,
        }
        if self.builtin:
            data["builtin"] = self.builtin
        if self.path_label:
            data["path"] = self.path_label
        return data


def builtin_frontend_plugin_names() -> set[str]:
    return set(_BUILTIN_PLUGIN_FILES)


def resolve_frontend_plugin(name: str, *, builtin: str | None = None, path: str | None = None) -> ResolvedFrontendPlugin:
    if builtin:
        file_path = _BUILTIN_PLUGIN_FILES[builtin]
        path_label = None
    else:
        file_path = Path(path or "")
        path_label = file_path.name or str(file_path)

    script = file_path.read_text(encoding="utf-8")
    sha256 = hashlib.sha256(script.encode("utf-8")).hexdigest()
    return ResolvedFrontendPlugin(
        name=name,
        kind="line",
        sha256=sha256,
        script=script,
        builtin=builtin,
        path_label=path_label,
    )
