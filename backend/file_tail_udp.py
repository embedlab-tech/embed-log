from __future__ import annotations

import argparse
import codecs
import logging
import os
import socket
import time
from pathlib import Path


DEFAULT_POLL_INTERVAL = 0.2
DEFAULT_ENCODING = "utf-8"


def parse_udp_target(value: str) -> tuple[str, int]:
    if ":" not in value:
        raise argparse.ArgumentTypeError(f"expected HOST:PORT, got {value!r}")
    host, port_s = value.rsplit(":", 1)
    host = host.strip()
    if not host:
        raise argparse.ArgumentTypeError(f"target host is empty in {value!r}")
    try:
        port = int(port_s)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(f"target port must be integer in {value!r}") from exc
    if not (1 <= port <= 65535):
        raise argparse.ArgumentTypeError(f"target port out of range in {value!r}")
    return host, port


class FileUdpForwarder:
    def __init__(
        self,
        path: str | Path,
        host: str,
        port: int,
        *,
        from_start: bool = False,
        poll_interval: float = DEFAULT_POLL_INTERVAL,
        encoding: str = DEFAULT_ENCODING,
        errors: str = "replace",
    ):
        self.path = Path(path)
        self.host = host
        self.port = port
        self.from_start = from_start
        self.poll_interval = poll_interval
        self.encoding = encoding
        self.errors = errors

        self._sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self._fp = None
        self._decoder = None
        self._pending = ""
        self._identity: tuple[int, int] | None = None
        self._offset = 0
        self._opened_once = False

    def close(self) -> None:
        self._close_file()
        try:
            self._sock.close()
        except OSError:
            pass

    def _close_file(self) -> None:
        if self._fp is not None:
            try:
                self._fp.close()
            except OSError:
                pass
        self._fp = None
        self._decoder = None
        self._pending = ""
        self._identity = None
        self._offset = 0

    def _stat(self):
        return self.path.stat()

    def _open_file(self, *, start_at_end: bool) -> None:
        st = self._stat()
        fp = self.path.open("rb")
        if start_at_end:
            fp.seek(0, os.SEEK_END)
        self._fp = fp
        self._identity = (st.st_dev, st.st_ino)
        self._offset = fp.tell()
        self._decoder = codecs.getincrementaldecoder(self.encoding)(errors=self.errors)
        self._pending = ""
        self._opened_once = True

    def _ensure_file_open(self) -> None:
        if self._fp is not None:
            return
        try:
            self._stat()
        except FileNotFoundError:
            return
        start_at_end = not self.from_start and not self._opened_once
        self._open_file(start_at_end=start_at_end)

    def _reopen_if_needed(self) -> None:
        if self._fp is None:
            return
        try:
            st = self._stat()
        except FileNotFoundError:
            self._close_file()
            return

        identity = (st.st_dev, st.st_ino)
        if self._identity is None:
            return
        if identity != self._identity or st.st_size < self._offset:
            self._close_file()
            self._open_file(start_at_end=False)

    def _emit_line(self, line: str) -> None:
        payload = line.rstrip("\r").encode("utf-8", errors="replace")
        if not payload:
            return
        self._sock.sendto(payload, (self.host, self.port))

    def _forward_available(self) -> int:
        if self._fp is None:
            return 0
        sent = 0
        while True:
            chunk = self._fp.read(65536)
            if not chunk:
                break
            self._offset += len(chunk)
            text = self._decoder.decode(chunk)
            self._pending += text
            while True:
                nl = self._pending.find("\n")
                if nl < 0:
                    break
                raw = self._pending[:nl]
                self._pending = self._pending[nl + 1 :]
                line = raw.rstrip("\r")
                if line:
                    self._emit_line(line)
                    sent += 1
        return sent

    def poll_once(self) -> int:
        self._ensure_file_open()
        self._reopen_if_needed()
        return self._forward_available()

    def run(self) -> int:
        logging.info(
            "tailing %s -> udp://%s:%d  from_start=%s  poll_interval=%.3fs",
            self.path,
            self.host,
            self.port,
            self.from_start,
            self.poll_interval,
        )
        try:
            while True:
                self.poll_once()
                time.sleep(self.poll_interval)
        except KeyboardInterrupt:
            print("Interrupted.")
            return 0
        finally:
            self.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="embed-log tail-file",
        description="Tail a file and forward appended lines to a UDP port.",
        epilog=(
            "Examples:\n"
            "  embed-log tail-file app.log 127.0.0.1:6000\n"
            "  embed-log tail-file app.log 127.0.0.1:6000 --from-start\n"
            "  embed-log tail-file C:\\logs\\service.log 127.0.0.1:6000 --poll-interval 0.5\n"
            "\n"
            "Each complete line is sent as one UDP datagram.\n"
            "By default the command starts at EOF and forwards only new lines."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("path", help="file to tail")
    parser.add_argument("target", type=parse_udp_target, help="UDP target as HOST:PORT")
    parser.add_argument(
        "--from-start",
        action="store_true",
        help="read the existing file contents first instead of starting at EOF",
    )
    parser.add_argument(
        "--poll-interval",
        type=float,
        default=DEFAULT_POLL_INTERVAL,
        help=f"seconds between file polls (default: {DEFAULT_POLL_INTERVAL})",
    )
    parser.add_argument(
        "--encoding",
        default=DEFAULT_ENCODING,
        help=f"file encoding (default: {DEFAULT_ENCODING})",
    )
    return parser


def run_tail_file(args: argparse.Namespace) -> int:
    if args.poll_interval <= 0:
        print("error: --poll-interval must be > 0", file=os.sys.stderr)
        return 2
    host, port = args.target
    forwarder = FileUdpForwarder(
        args.path,
        host,
        port,
        from_start=args.from_start,
        poll_interval=args.poll_interval,
        encoding=args.encoding,
    )
    print(
        f"Tailing {Path(args.path)} -> udp://{host}:{port}  "
        f"from_start={args.from_start}  poll_interval={args.poll_interval:g}s"
    )
    return forwarder.run()


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return run_tail_file(args)


if __name__ == "__main__":
    raise SystemExit(main())
