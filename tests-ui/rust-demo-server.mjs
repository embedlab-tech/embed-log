import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import dgram from 'node:dgram';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '..');
const tmp = path.join(here, '.tmp');
const logs = path.join(tmp, 'logs');

const regressionMode = process.argv.includes('--regression');
const config = regressionMode ? path.join(here, 'config-regression.yml') : path.join(tmp, 'demo-e2e.yml');

fs.rmSync(tmp, { recursive: true, force: true });
fs.mkdirSync(logs, { recursive: true });

if (!regressionMode) {
  const demoConfig = `version: 1
server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log e2e
  timestamp_mode: absolute
logs:
  dir: ${JSON.stringify(logs).slice(1, -1)}
sources:
  - name: DUT
    label: DUT UART
    type: udp
    port: 16000
  - name: HOST
    label: Host Debug
    type: udp
    port: 16001
  - name: SENSORS
    label: Sensor Bus
    type: udp
    port: 16002
    parser:
      type: cbor-datagram
frontend_plugins:
  hex-coap:
    builtin: hex-coap
tabs:
  - label: Device
    panes:
      - source: DUT
        plugins: [hex-coap]
      - source: HOST
  - label: Sensors
    panes: [SENSORS]
`;
  fs.writeFileSync(config, demoConfig);
}

const children = [];

const udpSocket = dgram.createSocket('udp4');

function cborText(text) {
  const bytes = Buffer.from(text, 'utf8');
  if (bytes.length < 24) return Buffer.concat([Buffer.from([0x60 | bytes.length]), bytes]);
  if (bytes.length < 256) return Buffer.concat([Buffer.from([0x78, bytes.length]), bytes]);
  throw new Error(`test CBOR text too long: ${text}`);
}

function cborUint(n) {
  if (n < 24) return Buffer.from([n]);
  if (n < 256) return Buffer.from([0x18, n]);
  if (n < 65536) return Buffer.from([0x19, n >> 8, n & 0xff]);
  throw new Error(`test CBOR uint too large: ${n}`);
}

function cborMap(entries) {
  if (entries.length >= 24) throw new Error('test CBOR map too large');
  return Buffer.concat([
    Buffer.from([0xa0 | entries.length]),
    ...entries.flatMap(([key, value]) => [
      cborText(key),
      typeof value === 'number' ? cborUint(value) : cborText(String(value)),
    ]),
  ]);
}

function sendUdp(port, payload) {
  const buffer = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
  udpSocket.send(buffer, port, '127.0.0.1');
}

// ── CoAP hex messages (mirrors original deterministic_demo_traffic.py) ──

// Pre-generated CoAP hex payloads (15 variants; loops)
const COAP_HEX_LIST = [
  '40 01 00 00 00 00 00 00',
  '40 02 00 00 00 00 00 00',
  '40 01 00 01 00 00 00 00',
  '40 02 00 01 00 00 00 00',
  '44 01 00 00 00 00 00 00',
  '44 02 00 00 00 00 00 00',
  '62 01 00 00 00 00 00 00',
  '62 02 00 00 00 00 00 00',
  '40 01 00 00 00 00 00 01',
  '40 02 00 00 00 00 00 02',
  '40 01 00 02 00 00 00 00',
  '40 02 00 02 00 00 00 00',
  '44 01 00 02 00 00 00 00',
  '44 02 00 02 00 00 00 00',
  '62 01 00 02 00 00 00 00',
];

function coapHex(tick) {
  return COAP_HEX_LIST[tick % COAP_HEX_LIST.length];
}

// ── Original deterministic traffic (test content mode) ──

function _msg(src, tick, seq, kind, message) {
  const t = String(tick).padStart(3, '0');
  const s = String(seq).padStart(4, '0');
  return `TEST src=${src} tick=${t} seq=${s} kind=${kind} msg="${message}"`;
}

function regressionLines(src, tick) {
  let seq = 1; // per-tick seq counter
  const lines = [];

  lines.push(_msg(src, tick, seq, 'sync', `${src} synchronized step ${String(tick).padStart(3, '0')}`));
  seq++;

  if (tick % 5 === 0) {
    lines.push('<wrn> ' + _msg(src, tick, seq, 'warning', `${src} warning at tick ${String(tick).padStart(3, '0')}`));
    seq++;
  }
  if (tick % 7 === 0) {
    lines.push('<err> ' + _msg(src, tick, seq, 'error', `${src} error at tick ${String(tick).padStart(3, '0')}`));
    seq++;
  }
  if (tick % 13 === 0) {
    lines.push(_msg(src, tick, seq, 'filter-alpha', 'alpha filter target'));
    seq++;
  }
  if (tick % 17 === 0) {
    lines.push(_msg(src, tick, seq, 'filter-beta', 'beta filter target'));
    seq++;
  }
  if (tick % 9 === 0) {
    lines.push(`[${src}] ` + _msg(src, tick, seq, 'prefix-cleanup', 'duplicated source prefix'));
    seq++;
  }
  if (tick % 11 === 0) {
    lines.push(`[2026-01-01T00:00:${String(tick % 60).padStart(2, '0')}Z] ` + _msg(src, tick, seq, 'timestamp-cleanup', 'duplicated timestamp prefix'));
    seq++;
  }
  if (src === 'SENSOR_A' && tick % 8 === 4) {
    const hexMsg = COAP_HEX_LIST[tick % COAP_HEX_LIST.length];
    lines.push(_msg(src, tick, seq, 'coap-demo', `coap rx: frame AA 55 payload ${hexMsg}`));
    seq++;
  }
  return lines;
}

function regressionCoapLines(tick) {
  const lines = [];
  const hex = coapHex(tick);
  lines.push(`coap ${hex}`);
  if (tick % 3 === 0) {
    const hex2 = coapHex(tick + 3);
    lines.push(`coap ${hex2}`);
  }
  if (tick % 5 === 0) {
    const compact = coapHex(tick + 7).replace(/ /g, '');
    lines.push(`coap-compact ${compact}`);
  }
  return lines;
}

function regressionCborRecord(tick) {
  const t = String(tick).padStart(3, '0');
  return cborMap([
    ['kind', 'sync'],
    ['src', 'SENSOR_CBOR'],
    ['tick', tick],
    ['seq', 1],
    ['msg', `CBOR synchronized step ${t}`],
  ]);
}

let tick = 0;
function startRegressionTraffic() {
  const tickMs = Number.parseInt(process.env.DEMO_TEST_TICK_MS || '100', 10);
  const interval = Number.isFinite(tickMs) && tickMs > 0 ? tickMs : 100;
  const timer = setInterval(() => {
    const t = tick;
    tick += 1;

    // SENSOR_A -> port 6000
    for (const line of regressionLines('SENSOR_A', t)) {
      sendUdp(6000, line + '\n');
    }

    // SENSOR_B -> port 6001
    for (const line of regressionLines('SENSOR_B', t)) {
      sendUdp(6001, line + '\n');
    }

    // SENSOR_C -> port 6002
    for (const line of regressionLines('SENSOR_C', t)) {
      sendUdp(6002, line + '\n');
    }

    // SENSOR_D -> port 6004
    for (const line of regressionLines('SENSOR_D', t)) {
      sendUdp(6004, line + '\n');
    }

    // SENSOR_COAP -> port 6005 (bare hex lines)
    for (const line of regressionCoapLines(t)) {
      sendUdp(6005, line + '\n');
    }

    // SENSOR_CBOR -> port 6003 (CBOR datagrams)
    sendUdp(6003, regressionCborRecord(t));
  }, interval);
  children.push({ kill: () => clearInterval(timer) });
}

// ── Existing demo traffic ──

function startDeterministicTraffic() {
  const tickMs = Number.parseInt(process.env.DEMO_TEST_TICK_MS || '100', 10);
  const interval = Number.isFinite(tickMs) && tickMs > 0 ? tickMs : 100;
  const timer = setInterval(() => {
    const tickText = String(tick).padStart(3, '0');
    sendUdp(6000, `TEST src=DUT tick=${tickText} level=INFO message=deterministic-dut\n`);
    sendUdp(6001, `TEST src=HOST tick=${tickText} status=ok message=deterministic-host\n`);
    sendUdp(6002, cborMap([
      ['kind', 'test'],
      ['src', 'SENSORS'],
      ['tick', tick],
    ]));
    tick += 1;
  }, interval);
  children.push({ kill: () => clearInterval(timer) });
}

// ── Process management ──

function start(cmd, args) {
  const child = spawn(cmd, args, {
    cwd: repo,
    stdio: 'inherit',
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG || 'warn' },
  });
  children.push(child);
  child.on('exit', (code, signal) => {
    if (!shuttingDown && code !== 0 && signal !== 'SIGTERM') {
      console.error(`${cmd} exited with ${code ?? signal}`);
      process.exitCode = 1;
      shutdown();
    }
  });
  return child;
}

let shuttingDown = false;
function shutdown() {
  if (shuttingDown) return;
  shuttingDown = true;
  for (const child of children) child.kill('SIGTERM');
  setTimeout(() => process.exit(process.exitCode || 0), 500).unref();
}

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);
process.on('exit', () => {
  for (const child of children) child.kill('SIGTERM');
});

// ── Boot ──

// Boot the log server. Use an installed binary from EMBED_LOG_BIN when set
// (e.g. CI install-e2e job); fall back to `cargo run` for local development.
const serverArgs = ['run', '--config', config, '--frontend-dir', 'frontend', '--no-open-browser'];
if (process.env.EMBED_LOG_BIN) {
  start(process.env.EMBED_LOG_BIN, serverArgs);
} else {
  start('cargo', ['run', '--quiet', '--package', 'embed-log-cli', '--bin', 'embed-log', '--', ...serverArgs]);
}

setTimeout(regressionMode ? startRegressionTraffic : startDeterministicTraffic, 1500);

setInterval(() => {}, 1 << 30);
