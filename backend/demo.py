"""
Demo — connects to both inject ports and every 10 seconds:
  1. Writes a log marker (visible in the log file and browser UI)
  2. Sends "heap stat\\n" to the device over serial TX

Runs for 60 seconds then exits.

Run the log server first:
    python3 backend/server.py \\
        --source READER     uart:/dev/ttyFTDI_A \\
        --source CONTROLLER uart:/dev/ttyFTDI_B \\
        --inject READER     5001 \\
        --inject CONTROLLER 5002 \\
        --tab "Devices" READER CONTROLLER \\
        --ws-port 8080

Then in a separate terminal:
    python3 backend/demo.py
"""

import time
import threading
from log_client import LogClient

INTERVAL = 10   # seconds between each cycle
DURATION = 60   # total run time in seconds


def device_writer(name: str, host: str, port: int, color: str, stop: threading.Event) -> None:
    counter = 0
    with LogClient(host, port, source="demo", connect_timeout=30) as client:
        print(f"[demo] connected to {name} on {host}:{port}")
        while not stop.wait(INTERVAL):
            counter += 1
            # Log marker — written to file with timestamp and color
            client.marker(
                f"sending 'heap stat' command (cycle #{counter})",
                color=color,
            )
            # TX — sends the command over the serial port
            client.sendline("heap stat")


def main() -> None:
    devices = [
        {"name": "GWL LNK Reader",     "host": "127.0.0.1", "port": 5001, "color": "cyan"},
        {"name": "GWL LNK Controller", "host": "127.0.0.1", "port": 5002, "color": "magenta"},
    ]

    stop = threading.Event()

    threads = [
        threading.Thread(
            target=device_writer,
            kwargs={**d, "stop": stop},
            daemon=True,
            name=f"writer-{d['name']}",
        )
        for d in devices
    ]

    for t in threads:
        t.start()

    print(f"[demo] running for {DURATION} s — 'heap stat' sent every {INTERVAL} s to each device")
    try:
        time.sleep(DURATION)
    except KeyboardInterrupt:
        pass
    finally:
        stop.set()
        print("[demo] done")


if __name__ == "__main__":
    main()
