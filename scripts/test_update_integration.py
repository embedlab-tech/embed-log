#!/usr/bin/env python3
"""Verify embed-log self-update against a local fake GitHub Release API.

The fixture serves a newer tag, a checksummed target archive, and no external
network. The candidate executable is a valid copy with trailing fixture bytes,
so a successful update proves the running binary was atomically replaced.
"""

from __future__ import annotations

import argparse
import hashlib
import http.server
import json
import os
import platform
import shutil
import subprocess
import tarfile
import tempfile
import threading
from pathlib import Path

TAG = "v1.0.1"


def update_target() -> str:
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Linux" and machine in {"x86_64", "amd64"}:
        return "x86_64-unknown-linux-gnu"
    if system == "Darwin" and machine in {"arm64", "aarch64"}:
        return "aarch64-apple-darwin"
    if system == "Darwin" and machine == "x86_64":
        return "x86_64-apple-darwin"
    raise SystemExit(f"self-update fixture is unsupported on {system} {machine}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path, help="installed Linux x64 embed-log binary")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit(f"not an executable embed-log binary: {binary}")

    target = update_target()
    archive_name = f"embed-log-{target}.tar.gz"

    with tempfile.TemporaryDirectory(prefix="embed-log-update-test-") as directory:
        root = Path(directory)
        installed = root / "bin" / "embed-log"
        installed.parent.mkdir()
        shutil.copy2(binary, installed)
        installed.chmod(0o755)

        candidate = root / "candidate-embed-log"
        shutil.copy2(binary, candidate)
        with candidate.open("ab") as output:
            output.write(b"\nembed-log-update-integration-fixture\n")
        candidate.chmod(0o755)

        archive = root / archive_name
        with tarfile.open(archive, "w:gz") as tar:
            info = tar.gettarinfo(candidate, arcname="embed-log")
            info.mode = 0o755
            with candidate.open("rb") as source:
                tar.addfile(info, source)
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        sums = f"{checksum}  {archive_name}\n".encode()

        class ReleaseHandler(http.server.BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                base = f"http://127.0.0.1:{self.server.server_port}"
                if self.path == f"/repos/embedlab-tech/embed-log/releases/tags/{TAG}":
                    body = json.dumps(
                        {
                            "tag_name": TAG,
                            "html_url": f"{base}/release/{TAG}",
                            "assets": [
                                {"name": archive_name, "browser_download_url": f"{base}/assets/{archive_name}"},
                                {"name": "SHA256SUMS", "browser_download_url": f"{base}/assets/SHA256SUMS"},
                            ],
                        }
                    ).encode()
                elif self.path == f"/assets/{archive_name}":
                    body = archive.read_bytes()
                elif self.path == "/assets/SHA256SUMS":
                    body = sums
                else:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                pass

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), ReleaseHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            env = os.environ | {"EMBED_LOG_UPDATE_API_BASE": f"http://127.0.0.1:{server.server_port}"}
            check = subprocess.run(
                [str(installed), "update", "--check", "--version", TAG, "--json"],
                env=env,
                check=True,
                capture_output=True,
                text=True,
            )
            status = json.loads(check.stdout)
            assert status["update_available"] is True
            assert status["asset"] == archive_name

            subprocess.run(
                [str(installed), "update", "--version", TAG, "--yes"],
                env=env,
                check=True,
            )
        finally:
            server.shutdown()
            thread.join(timeout=5)

        assert hashlib.sha256(installed.read_bytes()).digest() == hashlib.sha256(candidate.read_bytes()).digest()
        assert not installed.with_suffix(".bak").exists()
        subprocess.run([str(installed), "version", "--json"], check=True, capture_output=True, text=True)

    print("self-update integration passed: checked release, verified checksum, and replaced executable")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
