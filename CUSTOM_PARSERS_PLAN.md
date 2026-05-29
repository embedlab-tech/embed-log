# Custom Parsers Plan

## Goal

Add support for non-human-readable log formats without changing the rest of the backend pipeline.

The primary target use case is Zephyr dictionary-based logging, where the device emits raw/binary log data and the backend decodes it into human-readable lines before storing and broadcasting them.

The desired behavior is:

```text
raw UART/UDP stream
  -> backend parser/decoder
  -> human-readable log lines
  -> existing .log files, WebSocket events, session export, UI
```

Everything downstream of decoding should continue to work as it does today.

## Current backend flow

Today the ingestion path is effectively:

```text
backend/sources/uart.py or backend/sources/udp.py
  -> decode UTF-8 / split lines
  -> SourceManager._on_source_line(str)
  -> queue
  -> timestamp formatting
  -> .log file
  -> WebSocket broadcast
  -> session.html export
```

The important detail is that `SourceManager` already expects text lines. That is a good boundary to preserve.

## Architectural direction

Introduce parsers as a per-source transport transformation layer, but keep the existing text-line boundary intact:

```text
raw transport source
  -> parser / adapter
  -> text LogSource
  -> SourceManager receives str lines
```

The backend core should continue to operate only on human-readable text lines.

This keeps the parser responsibility separate from:
- session management,
- log file writing,
- WebSocket protocol,
- frontend rendering,
- session HTML export.

## Source contracts

The current `LogSource` contract is already useful and should be preserved:

```python
class LogSource:
    def start(self, on_line: Callable[[str], None], stop, name) -> None:
        ...
```

Do not change that interface to sometimes emit bytes and sometimes emit text.

Instead, introduce a separate raw transport contract for parser-aware sources, for example:

```python
class RawLogSource:
    def start(self, on_chunk: Callable[[bytes], None], stop, name) -> None:
        ...

    def write(self, data: bytes) -> None:
        ...

    @property
    def supports_write(self) -> bool:
        ...
```

Then adapt raw sources back into the existing text-oriented `LogSource` interface.

This keeps:
- runtime code stable,
- parser concerns isolated,
- source capabilities such as UART TX passthrough preserved,
- future parser work from weakening the meaning of `LogSource`.

## Recommended abstraction

Add parser and raw-source modules, for example:

```text
backend/parsers/
  __init__.py
  base.py
  text.py
  command.py
  zephyr_dict.py
  factory.py

backend/sources/
  base.py
  raw_base.py
  raw_uart.py
  raw_udp.py
  parsed.py
```

A parser should be streaming-oriented because binary formats may not be line based:

```python
class StreamParser:
    def feed(self, data: bytes) -> list[str]:
        ...

    def flush(self) -> list[str]:
        ...
```

The adapter should wrap a `RawLogSource` and expose a normal `LogSource`:

```python
class ParsedSource(LogSource):
    def __init__(self, raw_source: RawLogSource, parser: StreamParser):
        self.raw_source = raw_source
        self.parser = parser

    def start(self, on_line, stop, name):
        def on_chunk(data: bytes):
            for line in self.parser.feed(data):
                on_line(line)

        self.raw_source.start(on_chunk, stop, name)

    def write(self, data: bytes):
        return self.raw_source.write(data)

    @property
    def supports_write(self):
        return self.raw_source.supports_write
```

The important follow-up requirement is that the adapter must also flush parser state when the source ends or reconnects cleanly, otherwise a trailing partial record can be lost.

## Transport semantics matter

The parser layer must preserve transport semantics, not just decode bytes.

There are two different transport shapes in the current backend:
- stream-oriented transport such as UART,
- packet/datagram-oriented transport such as UDP.

Those are not equivalent.

UART behavior today is effectively:
- accumulate bytes across reads,
- split on `\n`,
- strip trailing `\r`,
- decode with UTF-8 `errors="replace"`,
- trim trailing whitespace,
- drop empty lines,
- flush any trailing buffered bytes when the source stops or reconnects.

UDP behavior today is effectively:
- treat each datagram as a message boundary,
- decode only that datagram,
- split lines within the datagram,
- do not carry parser buffer from one datagram into the next,
- drop empty lines.

That distinction must remain true after the refactor.

A generic stream parser is safe for UART. It is not automatically safe for UDP unless the UDP adapter explicitly preserves per-datagram boundaries.

Recommended approach:
- keep UART as a raw byte stream source,
- keep UDP as a raw datagram source,
- ensure the UDP parsing path resets line framing at each datagram boundary, or uses a parser mode that treats each datagram independently.

Do not silently introduce cross-datagram buffering.

## Default text parser requirements

The default `TextParser` should preserve current user-visible behavior exactly enough that existing logs, tests, and UI behavior do not change.

Required behavior:
- input type is bytes,
- decode using UTF-8 with `errors="replace"`,
- split on newline boundaries,
- strip trailing `\r`,
- strip trailing whitespace from emitted lines to match current behavior,
- emit only non-empty lines,
- support `flush()` so trailing buffered text is emitted when appropriate.

For datagram-style transports, preserve current packet boundary semantics as described above.

## Configuration shape

Parsers should be configured per source.

Default text parser:

```yaml
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
    baudrate: 115200
    parser:
      type: text
```

If `parser` is omitted, `text` should be used to preserve current behavior.
That means the new parser layer becomes part of the normal ingestion path for all sources, and the existing text behavior is formalized as the explicit default parser rather than remaining an implicit transport-specific implementation.

Zephyr dictionary parser example:

```yaml
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
    baudrate: 115200
    parser:
      type: zephyr-dict
      dictionary: build/zephyr/log_dictionary.json
```

A more generic command-based parser may also be useful:

```yaml
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
    baudrate: 115200
    parser:
      type: command
      command:
        - python3
        - /path/to/zephyr/scripts/logging/dictionary/log_parser.py
        - --database
        - build/zephyr/log_dictionary.json
```

The command parser would pipe raw bytes to a subprocess and emit decoded stdout lines.

Internally, the config loader should stop collapsing sources into only `(name, spec)` tuples. Parser support needs a richer internal representation, for example a dict or dataclass carrying:
- source transport type,
- transport parameters,
- parser type,
- parser options,
- source label,
- inject/forward settings,
- future raw-capture settings.

Trying to tunnel parser config through `uart:/path@baud`-style strings will make validation and future changes harder.

## Command parser operational rules

A subprocess-based parser is viable, but it must have explicit lifecycle rules.

The plan should define:
- how stdin writes handle backpressure,
- whether child stderr is logged and at what level,
- what happens if the parser exits unexpectedly,
- whether restart is automatic and under what retry policy,
- how shutdown flush works,
- what happens to undecoded bytes if the parser dies,
- whether parser startup failure should fail the source or the whole app.

Without these rules, the command parser will be the highest-risk part of the design.

## Files likely to change

Expected backend changes:
- `backend/parsers/*` — new parser abstractions and implementations.
- `backend/sources/base.py` — keep the text `LogSource` contract explicit.
- `backend/sources/raw_base.py` — new raw transport contract.
- `backend/sources/uart.py` / `backend/sources/udp.py` or new raw variants — move hard-coded UTF-8 line decoding out of the transport layer.
- `backend/sources/parsed.py` — parser adapter from raw transport to text source.
- `backend/app.py` — build parser-enabled source objects.
- `backend/config/loader.py` — parse and validate `sources[].parser` using a richer source config shape.
- `backend/cli.py` — preserve CLI behavior; config-based parsers are likely enough initially.

The following should not need parser-specific logic:
- `backend/core/runtime.py`.
- `backend/session/*`.
- `backend/net/ws_server.py`.
- frontend modules.
- `utils/merge_logs.py` / session export.

## Where not to place parsing

Avoid putting custom decoding in:
- `SessionExporter` — too late; `.log` files would already contain raw/unreadable data.
- frontend code — logs should already be human-readable when stored and streamed.
- `_writer_loop()` — mixes decoding with persistence and broadcasting.
- `backend/parse.py` — that tool parses exported `session.html`, unrelated to live ingestion.
- session/manifest logic — parser choice is a source property, not a session concern.

## Verification requirements

This change needs behavior-parity tests before higher-level parsers are added.

At minimum, add tests for:
- UART text parsing parity with today's behavior,
- UDP datagram parsing parity with today's behavior,
- trailing-buffer `flush()` behavior,
- invalid parser config validation,
- parser defaulting when `parser` is omitted,
- UART write/TX behavior preserved through the adapter.

Only after those pass should command or Zephyr-specific parsers be added.

## Raw capture consideration

For binary formats such as Zephyr dictionary logging, it may be useful to optionally store the original raw stream as a sidecar artifact:

```yaml
sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0
    parser:
      type: zephyr-dict
      dictionary: build/zephyr/log_dictionary.json
    raw_capture: true
```

Potential artifacts:

```text
DUT.log   # decoded human-readable log
DUT.raw   # original binary stream
```

This is not required for the first implementation, but it would be useful for debugging parser bugs, dictionary mismatches, or later re-decoding.

If added later, raw capture should tap the raw transport bytes before parser transformation.

## Forwarding behavior

Currently `forward_ports` forward text log messages after they enter the normal source pipeline.

With parser support, forwarding should continue to mean decoded human-readable lines, not raw bytes.

If raw forwarding is needed, it should be added explicitly as a separate feature, for example:

```yaml
raw_forward_ports: [7001]
```

This avoids ambiguity between decoded log forwarding and raw transport forwarding.

## Recommended implementation phases

### Phase 1: Internal config shape and parser abstraction

- Introduce a richer internal source config representation instead of only `(name, spec)` tuples.
- Add `StreamParser` interface.
- Add `TextParser`.
- Add parser factory.
- Wire `parser: text` as the implicit default.
- Add tests that pin current UART and UDP text behavior.

### Phase 2: Raw transport boundary

- Introduce `RawLogSource`.
- Refactor UART into a raw stream source.
- Refactor UDP into a raw datagram-aware source path.
- Add `ParsedSource` adapter so runtime still receives text lines.
- Preserve write/TX behavior for UART.
- Implement parser `flush()` handling at source shutdown / reconnect boundaries.

### Phase 3: Command parser

- Add a generic subprocess-based parser.
- Pipe raw input bytes to parser stdin.
- Read decoded stdout lines and emit them to the normal pipeline.
- Handle subprocess lifecycle, restart/error reporting, backpressure, and shutdown deterministically.
- Add focused tests for parser death and restart behavior.

### Phase 4: Zephyr dictionary parser

- Prefer configuring Zephyr's existing parser through the command parser first.
- Only add native Zephyr-specific integration if the command path proves insufficient.
- Keep Zephyr-specific code isolated from runtime/session/frontend.

### Phase 5: Optional raw capture/raw forwarding

- Add sidecar raw stream capture if needed.
- Add explicit raw forwarding if needed.
## Main design principle

Keep this invariant:

```text
After parser decoding, the rest of embed-log behaves exactly as before.
```

That means `.log` files, WebSocket events, session manifests, exported HTML snapshots, and frontend behavior all continue to consume human-readable text lines.
