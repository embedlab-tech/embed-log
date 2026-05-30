from __future__ import annotations

import io
import json
import logging

import cbor2

from .base import StreamParser

LOGGER = logging.getLogger(__name__)


def _format_value(value) -> str:
    """Format a single CBOR-decoded value for key=value output."""
    if isinstance(value, str):
        return value
    if isinstance(value, bool):
        return "True" if value else "False"
    if isinstance(value, (int, float)):
        return str(value)
    if value is None:
        return "None"
    # bytes, list, dict, etc. → JSON string
    return json.dumps(value)


def _format_line(obj: dict) -> str:
    """Format a CBOR map into a deterministic key=value line."""
    pairs: list[str] = []
    for key in sorted(obj.keys()):
        val = obj[key]
        pairs.append(f"{key}={_format_value(val)}")
    return " ".join(pairs)


class CborDatagramParser(StreamParser):
    """Parser for CBOR-encoded UDP datagrams.

    Each call to *feed* receives one complete datagram payload containing one
    CBOR-encoded map.  The map is decoded and converted to a human-readable
    ``key=value`` text line with sorted keys.

    Because this parser is datagram-oriented there is no cross-call buffering
    and *flush* always returns ``[]``.
    """

    def feed(self, data: bytes) -> list[str]:
        if not data:
            return []

        stream = io.BytesIO(data)
        try:
            obj = cbor2.load(stream)
        except Exception:
            LOGGER.warning("malformed CBOR payload, dropping datagram (%d B)", len(data))
            return []

        # Reject trailing bytes — one complete CBOR object per datagram
        if stream.tell() != len(data):
            LOGGER.warning(
                "CBOR datagram contains %d trailing byte(s), dropping (%d B)",
                len(data) - stream.tell(),
                len(data),
            )
            return []

        if not isinstance(obj, dict):
            LOGGER.warning("unexpected top-level CBOR type %s, dropping datagram (%d B)",
                           type(obj).__name__, len(data))
            return []

        line = _format_line(obj)
        return [line]

    def flush(self) -> list[str]:
        return []
