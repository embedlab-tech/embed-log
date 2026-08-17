# Timestamp synchronization

Embed-log can synchronize lines that contain timestamps from more than one clock. This is useful when the capture host adds a system timestamp while the device application also emits its own timestamp.

## Clock domains

A line may contain one or more timestamp candidates. Candidates are not compared solely by their numeric value; each candidate has a clock domain:

- `system` — the timestamp assigned by the Embed-log host/backend, normally the timestamp used for live capture;
- `device` — a timestamp emitted by the device or application inside the log text, or the timestamp of an offline device-log import.

For example:

```text
08-17 08:49:17.384 [OUTSIDE] [2026-08-17T08:12:20.873Z]:<inf> eventlog: ...
```

contains both a host/system timestamp and a device timestamp.

The normal live line timestamp is the `system` candidate. RFC3339 timestamps found in the message body are added as `device` candidates. An imported JSON/Python-dict record whose timestamp is the only timestamp is marked as `device`.

## Supported timestamp forms

The synchronization parser recognizes:

```text
[2026-08-17T08:12:20.873Z] message
2026-08-17T08:12:20.873Z embedded in a message
{'timestamp': '2026-08-17T08:12:20.873000000Z', 'code': 'READY'}
```

RFC3339 offsets are supported as well as `Z`. Fractional seconds with up to nanosecond precision are accepted. The complete original line is retained as the message for imported structured records; the timestamp is used to create the numeric synchronization candidate.

## Resolution algorithm

When a line is selected:

1. If it has a `system` candidate, that candidate is preferred.
2. If it has no `system` candidate but has a `device` candidate, Embed-log searches other panes for the nearest `device` timestamp.
3. If a matching live line is found within the synchronization tolerance (currently 5 seconds), its `system` timestamp becomes the cross-pane synchronization anchor.
4. Other panes are then synchronized using that system anchor.
5. If no device match is found, synchronization falls back to the device domain rather than comparing device and system epoch values as if they were the same clock.

This means that selecting an imported device-only record can locate the corresponding live host line and then synchronize the rest of the session using the host timestamp from that line.

A line with both clocks always prefers its system timestamp when it is selected directly. A target pane without a system candidate can use the selected line's device candidate as a local fallback.

## Ordering and tolerance

Imported records are stably sorted by their absolute timestamp because synchronization searches the imported pane chronologically. The source's own sequence field remains part of the displayed original record and is not used as a clock.

Timestamp values are accepted literally. For example, a device record with `1970-01-01T00:00:02.562Z` is not rejected or normalized; it is treated as the earliest timestamp and therefore appears first on the device timeline.

The current cross-domain matching tolerance is 5 seconds. Exact and near-exact device timestamp matches are therefore supported without requiring equal millisecond values.

## Offline import

Import a device dump into a saved session with:

```bash
embed-log sessions import latest \
  --dir logs \
  --file device.log \
  --source DEVICE_LOG \
  --tab "Device log" \
  --label "Device UART"
```

The importer:

- copies the original file into the session;
- adds a new source and tab to the manifest;
- appends canonical records to `combined.jsonl`;
- marks their timestamp domain as `device`;
- preserves the complete original record as the message;
- sorts the imported records by timestamp for synchronization.

After importing, regenerate the self-contained report:

```bash
embed-log sessions export latest --dir logs --format html --output session.html
```

## Limitations

- A device-only record cannot be mapped to the host timeline if no other pane contains a matching device timestamp within the tolerance. In that case it remains synchronized in the device domain only.
- Device timestamps in arbitrary text are recognized when they are valid RFC3339 timestamps. Non-RFC3339 device clock formats need an additional parser rule.
- The browser's existing per-pane ad-hoc file picker is a view-only import; persistent session import should use `sessions import`.
