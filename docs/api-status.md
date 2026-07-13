# Status and capabilities API

`GET /api/v1/status` is a lightweight readiness and capability-discovery endpoint. Use it for process orchestration, CI startup checks, and deciding whether an already-running Embed-log instance has the required sources.

It does not require opening a WebSocket. The control WebSocket `hello` message remains the authoritative interactive control handshake.

## Response schema

HTTP status: `200 OK`

```json
{
  "ok": true,
  "api_version": "v1",
  "version": "1.0.0",
  "session_id": "2026-07-12_09-45-00",
  "control_api": true,
  "sources": {
    "NATIVE_SIM_UART": {
      "type": "uart",
      "label": "Native simulator",
      "writable": true,
      "available": true,
      "stats": {
        "maxsize": 1000,
        "dequeued": 42,
        "bytes": 4096
      }
    }
  }
}
```

| Field | Type | Meaning |
|---|---|---|
| `ok` | boolean | Endpoint successfully served a ready Embed-log process. |
| `api_version` | string | REST status schema version. |
| `version` | string | Running Embed-log package version. |
| `session_id` | string or null | Active session identifier, when available. |
| `control_api` | boolean | Whether `/api/v1/control` WebSocket control is enabled. |
| `sources` | object | Source ID keyed capability map. |
| `sources.<id>.type` | string | Source kind, such as `uart`, `file`, `udp`, or `network_capture`. |
| `sources.<id>.label` | string | Human-facing source label. |
| `sources.<id>.writable` | boolean | Whether UART TX/control writes are supported. |
| `sources.<id>.available` | boolean | Source is configured in this running server. It is not a hardware-link health probe. |
| `sources.<id>.stats` | object or null | Runtime queue/byte counters for the source. |

## Harness example

```bash
curl -fsS http://127.0.0.1:8080/api/v1/status
```

A harness can require source IDs before adopting an existing server:

```bash
curl -fsS http://127.0.0.1:8080/api/v1/status |
  jq -e '.ok and .sources.NATIVE_SIM_UART and .sources.PYTEST'
```
