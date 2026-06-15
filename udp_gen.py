#!/usr/bin/env python3
"""Send simulated log lines over UDP for testing embed-log.

Usage:
    python udp_gen.py [--port 6000] [--source DUT] [--rate 5]

Sends one log line every `rate` seconds (default: 0.5s) to localhost:<port>.
Press Ctrl+C to stop.
"""

import argparse
import socket
import time
import random
from datetime import datetime

LEVELS = ["INFO", "DEBUG", "WARN", "ERROR"]
MODULES = ["main", "sensor", "wifi", "ble", "ota", "fs", "net", "hal"]
MESSAGES = [
    "boot complete, firmware v2.1.0",
    "sensor reading: temperature={:.1f}C humidity={:.0f}%",
    "WiFi connected, RSSI={}dBm ip=192.168.1.{}",
    "BLE peripheral advertising, addr=AA:BB:CC:DD:{:02X}:{:02X}",
    "OTA update check: no new firmware",
    "SPI flash: read {} bytes at 0x{:06X}",
    "MQTT publish topic=/sensors/temp payload={{\"t\":{:.1f}}}",
    "NTP sync: offset={}ms stratum=2",
    "watchdog fed, uptime={}s",
    "GPIO interrupt on pin {}, edge=rising",
    "I2C bus error: device 0x{:02X} NACK",
    "heap: free={}KB allocated={}KB fragmentation={:.0f}%",
    "power: battery={:.0f}% charging={}",
    "log buffer flushed, {} entries written",
    "command received: {}",
    "telemetry batch: {} points queued",
]


def generate_line(seq: int) -> str:
    level = random.choice(LEVELS)
    module = random.choice(MODULES)
    template = random.choice(MESSAGES)

    # Fill in random values for the template
    try:
        if "{:.1f}" in template and "temperature" in template:
            msg = template.format(20 + random.random() * 15, 40 + random.random() * 40)
        elif "RSSI" in template:
            msg = template.format(-30 - random.randint(0, 60), random.randint(100, 200))
        elif "addr=" in template:
            msg = template.format(random.randint(0, 255), random.randint(0, 255))
        elif "read" in template and "bytes" in template:
            msg = template.format(random.randint(64, 4096), random.randint(0, 0xFFFFFF))
        elif "MQTT" in template:
            msg = template.format(20 + random.random() * 15)
        elif "offset=" in template:
            msg = template.format(random.randint(-50, 50))
        elif "uptime=" in template:
            msg = template.format(seq * 2)
        elif "pin" in template:
            msg = template.format(random.randint(0, 39))
        elif "0x{:02X}" in template and "NACK" in template:
            msg = template.format(random.randint(0x10, 0x7F))
        elif "heap:" in template:
            free = random.randint(80, 200)
            alloc = random.randint(40, 150)
            frag = random.random() * 30
            msg = template.format(free, alloc, frag)
        elif "battery" in template:
            msg = template.format(random.randint(20, 100), random.choice(["true", "false"]))
        elif "entries" in template:
            msg = template.format(random.randint(10, 500))
        elif "command" in template:
            msg = template.format(random.choice(["reboot", "ping", "status", "reset", "config"]))
        elif "points" in template:
            msg = template.format(random.randint(1, 50))
        else:
            msg = template
    except (IndexError, KeyError):
        msg = template

    ts = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    return f"[{ts}] [{level}] [{module}] {msg}"


def main():
    parser = argparse.ArgumentParser(description="UDP log generator for embed-log")
    parser.add_argument("--port", type=int, default=6000, help="UDP port (default: 6000)")
    parser.add_argument("--source", default="DUT", help="Source name label (default: DUT)")
    parser.add_argument("--rate", type=float, default=0.5, help="Seconds between lines (default: 0.5)")
    parser.add_argument("--burst", type=int, default=1, help="Lines per send (default: 1)")
    args = parser.parse_args()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    target = ("127.0.0.1", args.port)

    print(f"Sending logs to UDP {target[0]}:{target[1]} every {args.rate}s")
    print(f"Source: {args.source}  Burst: {args.burst}")
    print("Ctrl+C to stop\n")

    seq = 0
    try:
        while True:
            for _ in range(args.burst):
                line = generate_line(seq)
                sock.sendto((line + "\n").encode("utf-8"), target)
                seq += 1
            time.sleep(args.rate)
    except KeyboardInterrupt:
        print(f"\nSent {seq} log lines.")
    finally:
        sock.close()


if __name__ == "__main__":
    main()
