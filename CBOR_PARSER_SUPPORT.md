# CBOR Parser Support Plan

## Goal

Add a real binary parser example using **CBOR per UDP datagram** to prove that the new parser architecture supports non-human-readable formats without changing the rest of the backend pipeline.

The demonstration should show this flow:

```text
UDP datagram carrying CBOR bytes
  -> CBOR parser
  -> human-readable log line
  -> existing runtime/session/ws/export/UI pipeline
```

The primary objective is not just feature delivery, but to establish a clean, well-tested reference implementation for future custom parsers.

## Scope

This plan covers:
- one new parser type: `cbor-datagram`
- one practical binary demo path: UDP datagrams
- config support for selecting the parser
- deterministic test/demo traffic generation
- unit tests and targeted integration verification

This plan does not cover:
- UART framing for CBOR
- command-based parsers
- Zephyr dictionary parsing
- raw capture / raw forwarding

## Design constraints

The implementation must preserve these invariants:
- runtime and `SourceManager` continue to receive only decoded `str` lines
- session logs remain human-readable text
- WebSocket payloads remain unchanged
- session export and replay continue to operate on decoded text lines
- UDP datagram boundaries remain meaningful and must not be merged across parser state

Because this parser is datagram-based:
- each UDP datagram is treated as one independent CBOR payload
- `flush()` should normally emit nothing
- no cross-datagram buffering may be introduced

## Proposed parser contract

Parser config:

```yaml
sources:
  - name: SENSOR_A
    type: udp
    port: 6000
    parser:
      type: cbor-datagram
```

Expected payload shape for the demo should be deliberately small and stable, for example:

```python
{
    "level": "INFO",
    "event": "temp",
    "value": 23.4,
    "unit": "C",
    "tick": 17,
}
```

Example decoded line:

```text
level=INFO event=temp value=23.4 unit=C tick=17
```

The formatter should be deterministic so tests can assert exact substrings such as `event=temp` and `tick=017` if zero-padding is chosen.

## Dependency choice

Use an existing Python CBOR library rather than implementing CBOR decoding manually.

Recommended library:
- `cbor2`

Why:
- mature and widely used
- minimal API surface
- suitable for embedded / tooling use cases
- straightforward encode/decode for both parser and deterministic demo generator

Required dependency updates:
- `pyproject.toml`
- `requirements.txt`

## Files expected to change

Backend/parser files:
- `backend/parsers/factory.py`
- `backend/parsers/__init__.py`
- `backend/parsers/cbor_datagram.py` (new)
- `backend/config/loader.py`

Demo/test generation:
- `utils/deterministic_demo_traffic.py`
- optionally `run_demo.sh`
- optionally `embed-log.demo.yml` or a dedicated demo config for CBOR

Tests:
- new parser unit tests under `tests/`
- config loader tests under `tests/test_config_loader.py`
- targeted runtime/integration tests if needed
- optional Playwright regression coverage if a visible demo path is changed

## Execution phases

### Phase 1: Lock the parser contract with tests first

Before implementing the parser, define exactly what decoded output should look like.

Add unit tests that describe the desired behavior:
- valid CBOR map decodes into one expected text line
- numeric and string fields are rendered deterministically
- unsupported top-level CBOR types are rejected cleanly
- malformed CBOR bytes are rejected cleanly
- empty/invalid payload handling is explicit and tested
- `flush()` returns `[]`
- one datagram produces at most one decoded line unless multi-line behavior is intentionally supported

Acceptance criteria:
- tests define the exact line format
- no implementation details are guessed later

### Phase 2: Add config support with validation tests

Extend parser config validation to accept:

```yaml
parser:
  type: cbor-datagram
```

Add tests for:
- `parser.type: cbor-datagram` accepted on UDP source
- omitted parser still defaults to `text`
- unsupported parser type still fails
- invalid parser config shape fails
- if the implementation restricts this parser to UDP only, that rule must be validated explicitly

Acceptance criteria:
- config errors are precise and deterministic
- existing `text` parser behavior remains unchanged

### Phase 3: Implement the parser

Add `backend/parsers/cbor_datagram.py`.

Implementation requirements:
- use `cbor2.loads(data)` on each datagram payload
- expect one complete CBOR object per datagram
- validate the decoded object shape
- format a stable, human-readable line
- do not buffer across `feed()` calls
- `flush()` returns `[]`

Error handling must be explicit.

Decide and test one policy for malformed payloads:
- either drop invalid payloads with a logged warning, or
- emit a readable diagnostic line, or
- raise and let the caller handle it

Recommended approach for this project:
- do not crash the whole runtime on one malformed datagram
- log a warning and drop the bad payload

Acceptance criteria:
- parser is deterministic
- parser does not weaken UDP boundary semantics
- parser failure policy is covered by tests

### Phase 4: Wire parser factory and source construction

Update parser factory to construct `cbor-datagram`.

Add tests proving that:
- config selects the correct parser
- default text parser path still works
- UDP + `cbor-datagram` builds successfully

Acceptance criteria:
- no existing source behavior regresses
- parser selection is entirely config-driven

### Phase 5: Add deterministic CBOR demo traffic generation

Extend or add a demo generator so it can emit CBOR-encoded UDP payloads.

Recommended approach:
- reuse `utils/deterministic_demo_traffic.py`
- add a mode or flag that sends CBOR datagrams for one or more sources
- generate deterministic records with the same semantic content the UI tests already rely on

Example deterministic record:

```python
{
    "level": "INFO",
    "source": "SENSOR_A",
    "event": "filter-alpha",
    "tick": 11,
    "kind": "filter-alpha",
}
```

This should decode into a line still containing the tokens that tests wait for, such as:
- `kind=filter-alpha`
- `tick=011`

That preserves existing test strategy while proving that the source bytes are binary.

Acceptance criteria:
- generated traffic is binary on the wire
- decoded lines remain deterministic and test-friendly

### Phase 6: Add focused backend tests

Add or update backend tests for:
- parser factory builds `cbor-datagram`
- parser output formatting
- malformed CBOR handling
- config validation for `cbor-datagram`
- end-to-end UDP source -> parser -> text line path at backend level if practical

If feasible, add a small integration test that:
- starts a UDP source with `cbor-datagram`
- sends one CBOR datagram
- verifies the resulting `.log` line or captured runtime line is decoded text

Acceptance criteria:
- tests exercise behavior, not implementation plumbing
- malformed input path is covered
- decoded output path is covered

### Phase 7: Add optional Playwright verification only if the visible demo changes

If the CBOR demo becomes part of the standard deterministic UI path, add or adapt a Playwright test that proves:
- the UI still renders decoded lines
- exported HTML still contains decoded text
- no binary/raw bytes leak into the frontend

If the CBOR demo is only a backend/example path and does not change default UI demo behavior, backend tests are sufficient.

Acceptance criteria:
- visible user behavior is verified when changed
- UI tests remain deterministic

## Unit test emphasis

The most important part of this feature is test coverage.

Minimum required test categories:

1. **Parser decoding tests**
   - valid CBOR map
   - missing required field
   - wrong field type
   - malformed CBOR bytes
   - unsupported top-level type

2. **Parser formatting tests**
   - deterministic field ordering
   - deterministic numeric/string formatting
   - deterministic tick formatting if used

3. **Parser lifecycle tests**
   - `feed()` emits one decoded line per valid datagram
   - `flush()` emits `[]`
   - no cross-datagram buffering

4. **Config tests**
   - parser accepted
   - parser rejected when invalid
   - parser default remains `text`

5. **Integration tests**
   - UDP bytes become decoded text lines
   - downstream consumer sees only decoded text

Tests should prefer exact logical assertions such as:
- line contains `kind=filter-alpha`
- line contains `tick=011`
- no exception escapes on malformed datagram

Tests should avoid asserting incidental formatting that is likely to churn without semantic value.

## Recommended output format policy

For maintainability, choose one formatting rule and keep it boring.

Recommended decoded line style:
- fixed key order
- `key=value` pairs separated by spaces
- no pretty-printing
- no JSON re-encoding in the line

Example:

```text
source=SENSOR_A level=INFO kind=filter-alpha event=temp value=23.4 unit=C tick=011
```

This is easy to:
- read in plain logs
- filter in the UI
- assert in backend tests
- preserve in exports

## Suggested implementation order

1. Add failing parser unit tests
2. Add failing config validation tests
3. Add `cbor2` dependency
4. Implement `CborDatagramParser`
5. Wire parser factory
6. Make config tests pass
7. Add deterministic CBOR traffic generation
8. Add backend integration test
9. Run targeted backend tests
10. Run Playwright tests only if demo/UI-visible paths changed

## Verification checklist

Before considering the feature complete:
- parser unit tests pass
- config loader tests pass
- backend integration tests pass
- default text parser tests still pass
- existing UDP text-mode behavior still passes unchanged
- if UI-visible demo paths changed, Playwright coverage passes

## Main principle

Keep the demonstration useful, not clever.

The CBOR example should prove:
- binary data can enter the system
- a parser can decode it cleanly
- the rest of `embed-log` does not need to care

If that is true and the tests are strong, the feature will serve as a reliable template for future custom parsers.
