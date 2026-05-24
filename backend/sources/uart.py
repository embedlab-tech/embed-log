from __future__ import annotations

import logging
import threading
from typing import Optional

import serial

from .base import LogSource


class UartSource(LogSource):
    def __init__(self, port: str, baudrate: int = 115200):
        self.port = port
        self.baudrate = baudrate
        self._ser: Optional[serial.SerialBase] = None
        self._ser_lock = threading.Lock()

    @property
    def supports_write(self) -> bool:
        return True

    def write(self, data: bytes) -> None:
        with self._ser_lock:
            if self._ser is None or not self._ser.is_open:
                raise serial.SerialException("serial port not open — cannot send TX data")
            self._ser.write(data)

    def start(self, on_line, stop, name):
        threading.Thread(
            target=self._run, args=(on_line, stop, name),
            daemon=True, name=f"{name}-uart",
        ).start()

    def _run(self, on_line, stop, name):
        while not stop.is_set():
            try:
                with serial.serial_for_url(self.port, baudrate=self.baudrate, timeout=0.01) as ser:
                    logging.info("[%s] opened serial %s @ %d", name, self.port, self.baudrate)
                    with self._ser_lock:
                        self._ser = ser
                    try:
                        buf = b""
                        while not stop.is_set():
                            raw = ser.read(65536)
                            if raw:
                                buf += raw
                                while b"\n" in buf:
                                    raw_line, buf = buf.split(b"\n", 1)
                                    clean = raw_line.rstrip(b"\r").decode("utf-8", errors="replace").rstrip()
                                    if clean:
                                        on_line(clean)
                            else:
                                stop.wait(0.005)
                    finally:
                        with self._ser_lock:
                            self._ser = None
                        if buf.strip():
                            on_line(buf.rstrip(b"\r").decode("utf-8", errors="replace").rstrip())
            except serial.SerialException as exc:
                logging.warning("[%s] serial error: %s — retrying in 3 s", name, exc)
                stop.wait(3)
