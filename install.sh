#!/bin/sh
set -eu

REPO="${EMBED_LOG_REPO:-embedlab-tech/embed-log}"
BIN="embed-log"
VERSION="${EMBED_LOG_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

msg() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"
}

is_interactive() {
  [ "${EMBED_LOG_NO_PROMPT:-0}" != "1" ] && [ -r /dev/tty ] && [ -w /dev/tty ]
}

prompt_yes_no() {
  question="$1"
  default_answer="${2:-Y}"

  if ! is_interactive; then
    [ "$default_answer" = "Y" ]
    return
  fi

  case "$default_answer" in
    Y) suffix="[Y/n]" ;;
    N) suffix="[y/N]" ;;
    *) suffix="[y/n]" ;;
  esac

  while :; do
    printf '%s %s ' "$question" "$suffix" >/dev/tty
    IFS= read -r answer </dev/tty || answer=""
    case "$answer" in
      "") [ "$default_answer" = "Y" ]; return ;;
      y|Y|yes|YES|Yes) return 0 ;;
      n|N|no|NO|No) return 1 ;;
      *) printf 'Please answer y or n.\n' >/dev/tty ;;
    esac
  done
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

select_profile() {
  if [ -n "${PROFILE:-}" ]; then
    printf '%s\n' "$PROFILE"
    return
  fi

  shell_name="$(basename "${SHELL:-sh}")"
  case "$shell_name" in
    zsh) printf '%s\n' "$HOME/.zshrc" ;;
    bash) printf '%s\n' "$HOME/.bashrc" ;;
    *) printf '%s\n' "$HOME/.profile" ;;
  esac
}

add_install_dir_to_path() {
  profile="$(select_profile)"
  touch "$profile"
  path_line="export PATH=\"$INSTALL_DIR:\$PATH\""
  if ! grep -F "$path_line" "$profile" >/dev/null 2>&1; then
    {
      printf '\n# Added by embed-log installer\n'
      printf '%s\n' "$path_line"
    } >> "$profile"
  fi
  msg "Added $INSTALL_DIR to PATH in $profile."
  msg "Restart your shell or run:"
  msg ""
  msg "  export PATH=\"$INSTALL_DIR:\$PATH\""
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

# Allow overriding the download base URL (mirrors, local staging, air-gapped installs).
if [ -n "${EMBED_LOG_BASE_URL:-}" ]; then
  base_url="$EMBED_LOG_BASE_URL"
fi

msg "Embed-log Installer"
msg "  Embedded log viewer and collection CLI."
msg ""
msg "Target: $target"
msg "Install directory: $INSTALL_DIR"
msg ""

if is_interactive; then
  msg "Choose an action:"
  msg ""
  msg "  y    Install $BIN (default)"
  msg "  n    Do nothing"
  msg ""
  if prompt_yes_no "Install $BIN now?" Y; then
    msg "Will install $BIN."
    msg ""
  else
    msg "Nothing changed."
    exit 0
  fi
fi

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

msg ""
msg "$BIN was installed successfully."
msg "Installed $BIN to $INSTALL_DIR/$BIN"

resolved_bin="$(command -v "$BIN" 2>/dev/null || true)"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) install_dir_on_path=1 ;;
  *) install_dir_on_path=0 ;;
esac

path_needs_update=0
if [ "$install_dir_on_path" != "1" ]; then
  path_needs_update=1
elif [ -n "$resolved_bin" ] && [ "$resolved_bin" != "$INSTALL_DIR/$BIN" ]; then
  path_needs_update=1
fi

if [ "$path_needs_update" = "1" ]; then
  msg ""
  if [ -n "$resolved_bin" ] && [ "$resolved_bin" != "$INSTALL_DIR/$BIN" ]; then
    msg "$BIN was installed, but your shell is not using that install yet."
    msg "Your shell currently resolves $BIN to: $resolved_bin"
  else
    msg "$BIN was installed, but $INSTALL_DIR is not on your PATH yet."
  fi

  update_path="ask"
  case "${EMBED_LOG_UPDATE_PATH:-}" in
    1|true|TRUE|yes|YES|y|Y) update_path="yes" ;;
    0|false|FALSE|no|NO|n|N) update_path="no" ;;
  esac

  if [ "$update_path" = "ask" ]; then
    if is_interactive && prompt_yes_no "Add $INSTALL_DIR to your PATH in $(select_profile) now?" Y; then
      update_path="yes"
    else
      update_path="no"
    fi
  fi

  if [ "$update_path" = "yes" ]; then
    add_install_dir_to_path
  else
    msg "Add it manually, for example:"
    msg ""
    msg "  export PATH=\"$INSTALL_DIR:\$PATH\""
    msg ""
    msg "Or re-run with automatic shell profile update enabled:"
    msg ""
    msg "  curl -fsSL https://github.com/$REPO/releases/latest/download/install.sh | EMBED_LOG_UPDATE_PATH=1 sh"
  fi
fi

msg ""
msg "Then run: $BIN"
