#!/bin/sh
set -eu

TARGET="${1:-}"
if [ -z "$TARGET" ]; then
  echo "usage: $0 <target-triple>" >&2
  exit 2
fi

BIN="embed-log"
BIN_PATH="${BIN_PATH:-target/release/$BIN}"
DIST_DIR="${DIST_DIR:-dist}"
ARCHIVE="$DIST_DIR/$BIN-$TARGET.tar.gz"

if [ ! -f "$BIN_PATH" ]; then
  echo "binary not found: $BIN_PATH" >&2
  echo "build it first, e.g. cargo build --locked --release --package embed-log-cli --bin embed-log" >&2
  exit 1
fi

mkdir -p "$DIST_DIR"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

cp "$BIN_PATH" "$tmp_dir/$BIN"
chmod 755 "$tmp_dir/$BIN"
tar -C "$tmp_dir" -czf "$ARCHIVE" "$BIN"

echo "$ARCHIVE"
