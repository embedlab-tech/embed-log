# Runtime UART dictionaries

`dictionary.stage`, `dictionary.activate` and `dictionary.status` use the existing
trusted-local `/api/v1/control` WebSocket. Control API must be enabled. This is an
explicit, per-source operation, **not automatic firmware or reboot detection**.
Do not expose the control API to untrusted networks.

## CLI

```sh
embed-log dictionary --instance bench --source UART_A --json stage target-log_dictionary.json
embed-log dictionary --instance bench --source UART_A --json status
embed-log dictionary --instance bench --source UART_A --json activate <dictionary_id> --expected-generation 0
```

`--url http://127.0.0.1:18080` can replace `--instance bench`. `source` identifies
one physical UART, never a tab or combined stream. `activate` accepts
`--parser-type zephyr-dict` (default) or `zephyr-dict-hex`.

## Python SDK

```python
with EmbedLogClient("ws://127.0.0.1:18080/api/v1/control") as client:
    target = client.stage_dictionary("UART_A", "target-log_dictionary.json")
    before = client.dictionary_status("UART_A")
    applied = client.activate_dictionary(
        "UART_A", target["dictionary_id"], before["generation"])
```

Staging uploads the **exact UTF-8 JSON file bytes**, validates Zephyr v3, and returns
`dictionary_id` = SHA256 of those bytes plus informational `build_id`. A git SHA
or `build_id` alone does not uniquely bind a dictionary to a firmware image.
Limits: 8 MiB per upload, 64 MiB staged per source for the process lifetime.
Invalid uploads leave the active parser unchanged. Staging does not activate.

## Protocol

All requests include `id` and `source_id`. Responses echo both, set `type` to
`<request type>.result`, and include `ok` (or `error` on failure).

* `dictionary.stage`: `dictionary_json` is a JSON **string containing file text**.
  Response `dictionary`: `{dictionary_id, build_id}`.
* `dictionary.status`: response `status`: `{source_id, generation, parser_type,
  dictionary, raw_file, raw_offset}`. Initial generation is 0.
* `dictionary.activate`: `dictionary_id`, required integer `expected_generation`,
  optional `parser_type`. Response `status` acknowledges application by the RX
  owner, with generation incremented. It does not merely acknowledge queuing.

Concurrent requests must use compare-and-swap generation. A stale generation
fails without changing the active parser. Timeout/disconnection is **not proof
of failure**: query status before retrying, because application and reply can
race a lost connection. Queued expired/cancelled requests are not applied later.

The RX owner swaps fully validated parsers between feeds, discarding any old
partial record and emitting an explicit boundary annotation. It never joins
bytes across generations. The acknowledged parser survives UART reconnect.
Other sources and UART TX are unaffected. `/api/v1/status` reports live dictionary
state alongside each source's counters.

## Artifacts and limits

Successful explicit staging enables raw capture for that source from its next
received bytes; raw capture is not enabled by ordinary startup. Bytes are
written **before decoding** to the session's `dictionaries/<source SHA256>-uart.bin`.
`raw_offset` is a byte offset, not a message count. This extra capture continues
until capture stops; provision disk space accordingly. A raw-write failure stops
that RX source rather than silently claiming a preserved stream.

The same directory retains immutable `<dictionary SHA256>.json` snapshots,
per-source `*-initial.json`, and `*-changes.jsonl` (previous/active state,
timestamps, offsets, explicit partial-record discard). Rotation copies staged
snapshots and creates a new raw stream/checkpoint. Existing sessions are not
cleaned up or restarted by activation. These files remain in the session tree;
HTML export itself is decoded text, not a raw-UART archive.

## Firmware update integration

1. Obtain each target dictionary from the **same compiled image/build job** as
   the firmware being installed. Bind the firmware and dictionary hashes to the
   target and build options; do not query a moving branch for a matching DB.
2. Stage each target dictionary before updating; keep current dictionaries
   active. This also preserves received bytes during the ambiguous interval.
3. Activate each source independently at an explicit, externally established
   transition boundary. Record the reason and generation. On failure/rollback,
   establish the actual running image before selecting a dictionary.
4. Preserve the session tree/raw files alongside exported HTML.

Firmware without an unambiguous dictionary/epoch marker cannot identify the
first record of each new image through update request/completion alone.
A consumer may explicitly reconcile **future** decoding after confirming
which image runs on each source, but must label the transition interval uncertain. This API
alone does not retroactively correct it, guarantee lossless logging, or safely
auto-select among dictionaries. Raw capture cannot recover device-side drops or
invent a missing boundary. TEXT-to-BIN may use the future DB from the outset
because dictionary RX preserves plain console text; this does not establish
safe switching between two different binary dictionaries.
