#!/usr/bin/env python3
"""Extended demo traffic generator for embed-log.

Sends realistic embedded device logs over UDP to multiple sources:
  - Port 6000 (DUT): main device UART simulation
  - Port 6001 (HOST): test controller / host logs
  - Port 6002 (SENSORS): CBOR-encoded sensor data

Usage:
    python3 demo_traffic.py [--rate 0.5]
"""

import argparse
import socket
import time
import random
import struct
import json

# ── CBOR encoding (minimal, no deps) ──

def cbor_encode_map(pairs):
    """Encode a dict as CBOR map."""
    result = bytearray()
    # Major type 5 (map) + length
    n = len(pairs)
    if n < 24:
        result.append(0xa0 | n)
    elif n < 256:
        result.append(0xb8)
        result.append(n)
    else:
        result.append(0xb9)
        result.extend(struct.pack('>H', n))
    for key, value in pairs:
        result.extend(cbor_encode(key))
        result.extend(cbor_encode(value))
    return bytes(result)

def cbor_encode(value):
    """Encode a Python value as CBOR."""
    if isinstance(value, str):
        encoded = value.encode('utf-8')
        n = len(encoded)
        if n < 24:
            return bytes([0x60 | n]) + encoded
        elif n < 256:
            return bytes([0x78, n]) + encoded
        else:
            return bytes([0x79]) + struct.pack('>H', n) + encoded
    elif isinstance(value, int):
        if 0 <= value < 24:
            return bytes([value])
        elif 0 <= value < 256:
            return bytes([0x18, value])
        elif 0 <= value < 65536:
            return bytes([0x19]) + struct.pack('>H', value)
        elif value < 0:
            # Negative int: major type 1
            if -24 <= value:
                return bytes([0x20 + (-1 - value)])
            else:
                return bytes([0x38, -1 - value])
        else:
            return bytes([0x1a]) + struct.pack('>I', value)
    elif isinstance(value, float):
        return bytes([0xfb]) + struct.pack('>d', value)
    elif isinstance(value, bool):
        return bytes([0xf5 if value else 0xf4])
    elif value is None:
        return bytes([0xf6])
    return b'\xf6'  # null fallback


# ── Log generators ──

DUT_LEVELS = ["INFO", "DEBUG", "WARN", "ERROR"]
DUT_MODULES = ["main", "sensor", "wifi", "ble", "ota", "fs", "net", "hal"]
DUT_MESSAGES = [
    "boot complete, firmware v2.1.0",
    "sensor reading: temperature={temp:.1f}C humidity={hum:.0f}%",
    "WiFi connected, RSSI={rssi}dBm ip=192.168.1.{ip}",
    "BLE peripheral advertising, addr=AA:BB:CC:DD:{b1:02X}:{b2:02X}",
    "OTA update check: no new firmware",
    "SPI flash: read {size} bytes at 0x{addr:06X}",
    "MQTT publish topic=/sensors/temp payload={{\"t\":{temp:.1f}}}",
    "NTP sync: offset={off}ms stratum=2",
    "watchdog fed, uptime={up}s",
    "GPIO interrupt on pin {pin}, edge=rising",
    "I2C bus error: device 0x{dev:02X} NACK",
    "heap: free={free}KB allocated={alloc}KB fragmentation={frag:.0f}%",
    "power: battery={bat:.0f}% charging={chg}",
    "log buffer flushed, {n} entries written",
    "command received: {cmd}",
    "telemetry batch: {n} points queued",
    # CoAP-like message (for hex-coap plugin)
    "coap send CON mid=0x{mid:04X} code=2.05 payload=hex:{hex}",
    "coap recv ACK mid=0x{mid:04X} code=2.05 etag=0x{etag:08X}",
]

HOST_MESSAGES = [
    "pytest: test_{test} PASSED ({dur:.2f}s)",
    "pytest: test_{test} FAILED — assert {val} != {expected}",
    "ci: build #{build} started (branch: {branch})",
    "ci: step {step}/8 running — {action}",
    "ci: artifact uploaded — {size}kB",
    "dut: serial timeout after {ms}ms, retrying",
    "dut: firmware flash complete ({size}kB in {dur:.1f}s)",
    "dut: heartbeat OK (latency={ms}ms)",
    "log: session rotated — {n} lines captured",
]

TEST_NAMES = ["boot_sequence", "sensor_read", "wifi_connect", "ota_update", "ble_pair", "power_mgmt", "flash_write"]
BRANCHES = ["main", "develop", "fix/uart-timeout", "feat/new-sensor"]

COAP_HEX_SAMPLES = [
    "45 01 00 01 11 22 33 44 b3 74 65 6d 70 ff 19 01 00",
    "65 45 00 01 11 22 33 44 ff 48 65 6c 6c 6f",
    "42 01 00 01 11 22",
]


def generate_dut_line(seq):
    level = random.choice(DUT_LEVELS)
    module = random.choice(DUT_MODULES)
    template = random.choice(DUT_MESSAGES)
    try:
        msg = template.format(
            temp=20 + random.random() * 15,
            hum=40 + random.random() * 40,
            rssi=-30 - random.randint(0, 60),
            ip=random.randint(100, 200),
            b1=random.randint(0, 255), b2=random.randint(0, 255),
            size=random.randint(64, 4096), addr=random.randint(0, 0xFFFFFF),
            off=random.randint(-50, 50),
            up=seq * 2 + random.randint(0, 100),
            pin=random.randint(0, 39),
            dev=random.randint(0x10, 0x7F),
            free=random.randint(80, 200), alloc=random.randint(40, 150),
            frag=random.random() * 30,
            bat=random.randint(20, 100), chg=random.choice(["true", "false"]),
            n=random.randint(1, 500),
            cmd=random.choice(["reboot", "ping", "status", "reset", "config", "flash"]),
            mid=random.randint(0, 0xFFFF),
            hex=random.choice(COAP_HEX_SAMPLES),
            etag=random.randint(0, 0xFFFFFFFF),
        )
    except (KeyError, IndexError, ValueError):
        msg = template

    ts = time.strftime("%H:%M:%S.") + f"{int(time.time() * 1000) % 1000:03d}"
    return f"[{ts}] [{level}] [{module}] {msg}"


def generate_host_line(seq):
    template = random.choice(HOST_MESSAGES)
    try:
        msg = template.format(
            test=random.choice(TEST_NAMES),
            dur=random.random() * 10,
            val=random.randint(0, 100),
            expected=random.randint(0, 100),
            build=random.randint(1000, 9999),
            branch=random.choice(BRANCHES),
            step=random.randint(1, 8),
            action=random.choice(["compile", "test", "lint", "deploy", "upload"]),
            size=random.randint(10, 500),
            ms=random.randint(5, 500),
            n=random.randint(10, 5000),
        )
    except (KeyError, IndexError):
        msg = template

    ts = time.strftime("%H:%M:%S.") + f"{int(time.time() * 1000) % 1000:03d}"
    return f"[{ts}] [HOST] {msg}"


def generate_sensor_cbor(seq):
    """Generate a CBOR-encoded sensor reading."""
    data = {
        "ts": int(time.time()),
        "seq": seq,
        "temp": round(20 + random.random() * 15, 2),
        "hum": round(40 + random.random() * 40, 1),
        "press": round(1013.25 + random.uniform(-5, 5), 2),
        "accel": {
            "x": round(random.uniform(-2, 2), 3),
            "y": round(random.uniform(-2, 2), 3),
            "z": round(random.uniform(8, 10), 3),
        },
        "batt_mv": random.randint(3200, 4200),
    }
    return cbor_encode_map(list(data.items()))


def main():
    parser = argparse.ArgumentParser(description="Extended demo traffic generator")
    parser.add_argument("--rate", type=float, default=0.5, help="Seconds between messages (default: 0.5)")
    parser.add_argument("--no-cbor", action="store_true", help="Disable CBOR sensor traffic")
    args = parser.parse_args()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    target = "127.0.0.1"

    print(f"Sending demo traffic every {args.rate}s")
    print(f"  DUT      → UDP :6000  (device UART simulation)")
    print(f"  HOST     → UDP :6001  (test controller)")
    if not args.no_cbor:
        print(f"  SENSORS  → UDP :6002  (CBOR sensor data)")
    print(f"  Ctrl+C to stop\n")

    seq = 0
    try:
        while True:
            # DUT: 1-3 lines per tick
            for _ in range(random.randint(1, 3)):
                line = generate_dut_line(seq)
                sock.sendto((line + "\n").encode("utf-8"), (target, 6000))
                seq += 1

            # HOST: 0-2 lines per tick (less frequent)
            for _ in range(random.randint(0, 2)):
                line = generate_host_line(seq)
                sock.sendto((line + "\n").encode("utf-8"), (target, 6001))
                seq += 1

            # SENSORS: CBOR datagram every tick
            if not args.no_cbor:
                cbor_data = generate_sensor_cbor(seq)
                sock.sendto(cbor_data, (target, 6002))

            time.sleep(args.rate)
    except KeyboardInterrupt:
        print(f"\nSent {seq} messages.")
    finally:
        sock.close()


if __name__ == "__main__":
    main()
