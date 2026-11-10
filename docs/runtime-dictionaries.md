# Runtime UART dictionaries

`dictionary.stage`, `dictionary.activate` and `dictionary.status` use the existing
trusted-local `/api/v1/control` WebSocket. Control API must be enabled. This is an
per-source operation, **not automatic firmware or reboot detection**. Optional
raw-line anchors are configured explicitly by the consumer.
Do not expose the control API to untrusted networks.

## YAML and live UI

The existing `database` field accepts either one path or an ordered list:

```yaml
sources:
  UART_A:
    type: uart
    path: /dev/ttyUSB0
    baud: 115200
    parser:
      type: zephyr-dict
      database:
        - firmware-A/log_dictionary.json
        - firmware-B/log_dictionary.json
```

`database: firmware-A/log_dictionary.json` remains supported and serializes as a
single string. Lists must be nonempty and contain only nonblank paths. All paths
are relative to the YAML file (absolute paths work too). Both `zephyr-dict` and
`zephyr-dict-hex` accept lists. The first dictionary is active at startup.

The backend validates and snapshots configured UART dictionaries on startup.
Missing/invalid alternatives are listed as unavailable, with an error, and cannot
be selected. A bad first dictionary keeps the existing startup-warning behavior
(described below); it does not silently choose another dictionary.

Each physical UART pane in the live UI shows a Dictionary selector and its actual
active path. Selection uses the same owner-acknowledged API as CLI/SDK, without a
restart. A rejected change keeps the previous binding. Reloading/rebuilding the UI
reads backend state; it does not reset the dictionary. Other UARTs are unaffected.
After restarting the logger, the first YAML entry is active again. Configured
snapshots do not change if the original file is subsequently edited.

There is **no fallback or guessing from decoded output**. A consumer can explicitly
arm one raw-line anchor as described below. A manual choice can be wrong even if
its output looks plausible. Earlier decoded logs are not rewritten.
Non-UART sources use the first path only; runtime selection is UART-only.

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
Invalid uploads leave the active parser unchanged. Plain staging does not activate.

## Optional raw UART anchor

```sh
embed-log dictionary --instance bench --source UART_A --json stage target.json \
  --activate-on-regex '^READY:target-specific-token$'
```

```python
client.stage_dictionary("UART_A", "target.json",
                        activate_on_regex=r"^READY:target-specific-token$")
```

The regex matches a complete **raw UTF-8 console line**, without its trailing
CR/LF, before any dictionary decoding. No marker spelling or platform knowledge
is built in. It does not match text reconstructed from a dictionary. This is not
the shadow-parser/timestamp heuristic: no reboot inference or boot-log replay is
performed. Bytes after the matching LF, even in the same UART read, use the
staged parser. The marker itself becomes a dictionary boundary annotation, remains
in raw capture and is recorded as `anchor_line` in history.

The producer must emit a distinct, newline-terminated marker directly on UART,
between records and **before the first dictionary record** of the new image.
Prefix it with LF/CRLF to separate it from preceding bytes. Serialize the write
against binary logging so the marker cannot interleave with a record. Bind the
expected token to the actual firmware and dictionary; a common readiness message
is not a build identity. Use an anchored regex and escape the token. The scanner
cannot distinguish a literal marker embedded inside binary payload from a real
marker: choose a sufficiently distinctive token and trust the producer/framing.

Only the binary `zephyr-dict` UART parser supports arming. Regexes are limited to
1024 characters and must not match an empty line. Compilation and owner-ACK
failures leave the active parser unchanged. One matcher may be armed per source;
a new regex replaces it, while staging without a regex leaves it alone. Arming
the active dictionary is rejected. `status.armed` is null or contains
`{dictionary_id, activate_on_regex}`. No UI change is required.

Potential marker lines span UART chunks and are bounded to 16 KiB and 60 seconds
from their first buffered byte (monotonic time, including silent UART). Nonmatches,
binary/control bytes, overlong lines and timed-out fragments pass through the old
parser; the matcher remains armed. This can briefly delay ordinary console text
while awaiting LF. Reconnect/rearming flush pending bytes through the old parser
and reset line matching; fragments are never joined across reconnects. A successful
explicit activation, including idempotent confirmation, disarms the matcher.

Auto-activation increments generation once and records `reason: "anchor"`,
`anchor_line` and `anchor_raw_offset` (the byte just after LF, or null if raw
capture failed). Explicit changes record `reason: "explicit"`. Both discard the
old parser's partial record; neither claims that a shadow parser was retained.
History-write failure prevents the switch and visibly disarms the matcher. Raw
write failure alone does not prevent activation. A missing, late or dropped marker
cannot recover earlier boot records; preserve raw/history and reconcile explicitly.

## Protocol

All requests include `id` and `source_id`. Responses echo both, set `type` to
`<request type>.result`, and include `ok` (or `error` on failure).

* `dictionary.stage`: `dictionary_json` is a JSON **string containing file text**.
  Optional `activate_on_regex` is a string; omitted/null keeps plain staging.
  Response `dictionary`: `{dictionary_id, build_id}`, plus boolean `armed`.
* `dictionary.status`: response `status`: `{source_id, generation, parser_type,
  dictionary, raw_file, raw_offset, raw_error, configured, armed}`. Initial generation is 0.
  `configured` contains `{path, dictionary, error}` for each YAML entry; unavailable
  entries have no `dictionary`. Uploaded dictionaries need not be in this list.
  `raw_file` is an absolute path when capture has opened the file.
* `dictionary.activate`: `dictionary_id`, required integer `expected_generation`,
  optional `parser_type`. Response `status` acknowledges application by the RX
  owner, with generation incremented. It does not merely acknowledge queuing.

Concurrent changes must use compare-and-swap generation. A stale generation
fails without changing the active parser, except that activating the already
active dictionary and parser type is an idempotent confirmation: it succeeds
without incrementing generation, swapping the parser or adding a history entry. Timeout/disconnection is **not proof
of failure**: query status before retrying, because application and reply can
race a lost connection. Queued expired/cancelled requests are not applied later.

The RX owner swaps fully validated parsers between feeds, discarding any old
partial record and emitting an explicit boundary annotation. It never joins
bytes across generations. The acknowledged parser survives UART reconnect.
Other sources and UART TX are unaffected. `/api/v1/status` reports live dictionary
state alongside each source's counters. Configuration views describe startup
YAML; use dictionary status for the active runtime binding.

A missing or invalid initial dictionary logs a warning with the source and path,
but does not abort the server or alter the configured parser's fallback behavior.
Runtime dictionary control is unavailable for that source until startup is fixed.

## Artifacts and limits

Successful explicit staging or an activation request enables raw capture for that
source from its next received bytes; merely listing dictionaries at startup does
not enable raw capture. Bytes are
written **before decoding** to the session's `dictionaries/<source SHA256>-uart.bin`.
`raw_offset` is a byte offset, not a message count. This extra capture continues
until capture stops; provision disk space accordingly. A raw-write failure disables
only raw capture: normal UART decoding continues, a visible warning is emitted,
and `raw_error` records the failure. The raw stream is incomplete from that point;
`raw_offset` counts only fully written chunks. Explicit staging retries capture;
a successful write clears the error.

The same directory retains immutable `<dictionary SHA256>.json` snapshots,
per-source `*-initial.json`, and `*-changes.jsonl` (previous/active state,
timestamps, offsets, explicit partial-record discard). When raw capture is enabled,
the first RX after rotation copies staged snapshots and creates a new raw
stream/checkpoint. Idle sources do not poll the filesystem for dictionaries;
reconnect refreshes the active snapshot if necessary. Existing sessions are not
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
