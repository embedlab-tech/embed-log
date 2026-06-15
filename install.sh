#!/bin/sh
set -eu

REPO="${EMBED_LOG_REPO:-krezolekcoder/embed-log}"
BIN="embed-log"
VERSION="${EMBED_LOG_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

msg() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"
}

download() {
  url="$1"
  out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$out"
  else
    err "curl or wget is required"
  fi
}

case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-gnu" ;;
  *) err "unsupported OS: $(uname -s). This installer supports macOS and Linux." ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) err "unsupported CPU architecture: $(uname -m)" ;;
esac

target="$arch-$os"
case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *) err "no prebuilt $BIN CLI release is currently published for $target" ;;
esac
archive="$BIN-$target.tar.gz"

case "$VERSION" in
  latest) base_url="https://github.com/$REPO/releases/latest/download" ;;
  *) base_url="https://github.com/$REPO/releases/download/$VERSION" ;;
esac

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

need_cmd tar
need_cmd grep

msg "Installing $BIN for $target"
msg "Downloading $archive"
download "$base_url/$archive" "$tmp_dir/$archive"
download "$base_url/SHA256SUMS" "$tmp_dir/SHA256SUMS"

msg "Verifying checksum"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp_dir" && grep "  $archive\$" SHA256SUMS | sha256sum -c -) >/dev/null
elif command -v shasum >/dev/null 2>&1; then
  (cd "$tmp_dir" && grep "  $archive\$" SHA256SUMS | shasum -a 256 -c -) >/dev/null
else
  err "sha256sum or shasum is required for checksum verification"
fi

tar -xzf "$tmp_dir/$archive" -C "$tmp_dir"

mkdir -p "$INSTALL_DIR"
cp "$tmp_dir/$BIN" "$INSTALL_DIR/$BIN"
chmod 755 "$INSTALL_DIR/$BIN"

msg "Installed $BIN to $INSTALL_DIR/$BIN"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    msg ""
    if [ "${EMBED_LOG_UPDATE_PATH:-0}" = "1" ]; then
      profile="${PROFILE:-}"
      if [ -z "$profile" ]; then
        shell_name="$(basename "${SHELL:-sh}")"
        case "$shell_name" in
          zsh) profile="$HOME/.zshrc" ;;
          bash) profile="$HOME/.bashrc" ;;
          *) profile="$HOME/.profile" ;;
        esac
      fi
      touch "$profile"
      if ! grep -F "$INSTALL_DIR" "$profile" >/dev/null 2>&1; then
        {
          printf '\n# Added by embed-log installer\n'
          printf 'export PATH="%s:$PATH"\n' "$INSTALL_DIR"
        } >> "$profile"
      fi
      msg "Added $INSTALL_DIR to PATH in $profile"
      msg "Open a new terminal, or run:"
      msg "  export PATH=\"$INSTALL_DIR:\$PATH\""
    else
      msg "Note: $INSTALL_DIR is not on your PATH. Add it, for example:"
      msg "  export PATH=\"$INSTALL_DIR:\$PATH\""
      msg ""
      msg "Or re-run the installer with automatic shell profile update enabled:"
      msg "  curl -fsSL https://github.com/$REPO/releases/latest/download/install.sh | EMBED_LOG_UPDATE_PATH=1 sh"
    fi
    ;;
esac

msg ""
msg "Try: $BIN --help"
