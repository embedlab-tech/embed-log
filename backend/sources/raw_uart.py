from __future__ import annotations

import logging
import threading
from typing import Optional

import serial

from .raw_base import RawLogSource


class RawUartSource(RawLogSource):
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

    def start(self, on_chunk, on_boundary, stop, name) -> None:
        threading.Thread(
            target=self._run,
            args=(on_chunk, on_boundary, stop, name),
            daemon=True,
            name=f"{name}-uart",
        ).start()

    def _run(self, on_chunk, on_boundary, stop, name) -> None:
        while not stop.is_set():
            try:
                with serial.serial_for_url(self.port, baudrate=self.baudrate, timeout=0.01) as ser:
                    logging.info("[%s] opened serial %s @ %d", name, self.port, self.baudrate)
                    with self._ser_lock:
                        self._ser = ser
                    try:
                        while not stop.is_set():
                            raw = ser.read(65536)
                            if raw:
                                on_chunk(raw)
                            else:
                                stop.wait(0.005)
                    finally:
                        with self._ser_lock:
                            self._ser = None
                        on_boundary()
            except serial.SerialException as exc:
                logging.warning("[%s] serial error: %s — retrying in 3 s", name, exc)
                stop.wait(3)
