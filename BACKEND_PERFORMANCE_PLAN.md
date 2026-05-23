# Backend performance improvement plan

This document tracks planned backend improvements for higher-rate multi-source logging. It was created after reviewing the Python backend because testing with 4 serial ports showed dropped frames.

## Likely backend bottlenecks found

1. **Disk write is per-line open/write/flush/close**
   - Location: `backend/core/runtime.py`
   - `_writer_loop()` opens the log file and flushes every single line.
   - This is expensive with multiple active serial ports.

2. **WebSocket broadcast is per-line**
   - Location: `backend/net/ws_server.py`
   - Every log line schedules an asyncio coroutine and sends one JSON message to every browser client.
   - A slow browser/UI can create a large amount of pending work.

3. **TCP stream/forward clients can block the writer**
   - `sendall()` is called directly from the source writer thread.
   - One slow client can delay file writing and WebSocket fanout for that source.

4. **No bounded queue / no drop counters / no stats**
   - Current queues are unbounded, so the server cannot clearly report when it is falling behind.
   - There is no `/stats` endpoint, queue-depth metric, dropped-message counter, or latency measurement.

5. **Serial read path is line-by-line**
   - Location: `backend/sources/uart.py`
   - Current implementation uses `ser.readline()`.
   - At high baud rates, especially with short lines, a bulk read + split strategy can drain the OS serial buffer faster.

## Step-by-step improvement plan

### Step 1 — Add observability first

Add counters per source:

- `rx_lines`
- `rx_bytes`
- `written_lines`
- `queue_depth`
- `queue_max_depth`
- `queue_oldest_age_ms`
- `ws_queued`
- `ws_dropped`
- `forward_dropped`
- `writer_errors`

Expose them through:

```text
GET /health
GET /stats
```

Goal: identify whether loss happens in serial reading, queueing, disk writing, WebSocket, TCP forwarding, or frontend rendering.

### Step 2 — Batch disk writing

Change the writer loop so each source keeps its log file open and writes in batches:

- open file once on source start
- write up to a configurable batch size, e.g. 100–1000 lines
- flush every configurable interval, e.g. 50–250 ms
- always flush on rotation and shutdown
- avoid `flush()` per line
- avoid `open()` per line

This is likely the highest-impact backend change.

### Step 3 — Decouple capture from slow sinks

Make the critical path:

```text
UART reader → source queue → disk writer
```

Then make WebSocket, inject stream clients, and forward sockets separate lower-priority fanout queues.

Disk capture should not block because:

- browser is slow
- TCP client is slow
- WebSocket queue is overloaded

If a UI/client cannot keep up, drop or coalesce UI messages, not serial capture.

### Step 4 — Batch WebSocket events

Instead of sending one WebSocket JSON message per log line, send batches:

```json
{
  "type": "events",
  "events": [
    { "type": "rx", "source_id": "A", "data": "...", "timestamp": "..." },
    { "type": "rx", "source_id": "B", "data": "...", "timestamp": "..." }
  ]
}
```

Flush batches every ~16–50 ms or when batch size reaches a limit.

Also add a max pending WebSocket queue size. If exceeded, drop UI events and increment `ws_dropped`, while still preserving disk logs.

### Step 5 — Make TCP forwarding non-blocking / buffered

Move `sendall()` out of the source writer thread.

For each forward/inject stream client:

- use a per-client queue
- set socket timeout or non-blocking mode
- disconnect or drop for slow clients
- count dropped forwarded messages

### Step 6 — Improve UART reader efficiency

Replace line-by-line `ser.readline()` with a bulk-read loop:

```text
read available bytes → append to buffer → split on '\n' → emit complete lines
```

This drains the OS serial buffer faster and reduces Python call overhead.

Also consider configurable serial options:

- larger OS receive buffer where supported
- lower timeout, e.g. 10–50 ms
- optional hardware flow control if devices support it

### Step 7 — Reduce per-line timestamp/format overhead

Currently every line does `datetime.now().astimezone()` and later multiple string formats.

Potential improvements:

- capture `time.time_ns()` or `datetime.now(timezone)` once
- format timestamp once in writer
- reuse formatted timestamp for file/WebSocket/client payloads where possible

### Step 8 — Add a reproducible performance test

Create a benchmark using pseudo-terminals or a synthetic source:

- 4 simulated UARTs
- configurable baud / lines per second
- sequence numbers in every line
- verify no missing sequence numbers in raw log files
- test with UI off/on
- test with slow WebSocket/forward clients

This should become the regression test for future performance changes.

## Immediate diagnostic recommendation

Before changing code, run the same 4-port test in three modes:

1. UI disabled: `ws_port: 0`
2. UI enabled but browser closed
3. UI enabled with browser open

Then compare raw session log files for missing sequence numbers.

Expected interpretation:

- If only mode 3 loses data, the bottleneck is likely WebSocket/frontend.
- If all modes lose data, optimize UART reading and disk batching first.
