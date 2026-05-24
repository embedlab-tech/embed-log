#!/usr/bin/env bash
#
# embed-log installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
#   ./install.sh
#
# Installs embed-log globally via pipx.  Requires Python >= 3.10.
# Works on macOS, Linux, and (via WSL/Git-Bash) Windows.

set -euo pipefail

# ─────────────────────────────────────────────────────────────────
# Config
# ─────────────────────────────────────────────────────────────────

REPO="krezolekcoder/embed-log"
BRANCH="main"
REPO_URL="https://github.com/${REPO}.git"
MIN_PY="3.10"
INSTALLER_VERSION="1.0.0"

# ─────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────

have_cmd() { command -v "$1" >/dev/null 2>&1; }

print_info() { printf '\033[36membed-log\033[0m %s\n' "$*"; }
print_ok()  { printf '  \033[32m✓\033[0m %s\n' "$*"; }
print_warn(){ printf '  \033[33m⚠\033[0m %s\n' "$*"; }
die() {
  printf '\n  \033[31m✕\033[0m %s\n' "$1" >&2
  exit 1
}

# ─────────────────────────────────────────────────────────────────
# Python version check
# ─────────────────────────────────────────────────────────────────

pick_python() {
  for c in python3 python; do
    if have_cmd "$c"; then
      echo "$c"
      return 0
    fi
  done
  return 1
}

ver_ge() {
  # Returns 0 (true) if $1 >= $2 using semver-aware sort
  [ "$(printf '%s\n' "$1" "$2" | sort -V | tail -n1)" = "$1" ]
}

print_info "Checking Python..."

PY="$(pick_python || true)"
[ -n "$PY" ] || die "\
Python not found (need >= ${MIN_PY}).

  Install Python 3.10 or later from:
    https://python.org

  Or via your system package manager:

    macOS : brew install python
    Ubuntu: sudo apt install python3 python3-pip python3-venv
    Fedora: sudo dnf install python3
    Arch  : sudo pacman -S python python-pip"

PY_VER="$("$PY" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")' 2>/dev/null)" || die "Failed to run Python interpreter ($PY)."

ver_ge "$PY_VER" "$MIN_PY" || die "\
Python ${PY_VER} is too old — version ${MIN_PY} or later is required.

  Upgrade Python on your system and try again."

print_ok "Python ${PY_VER} — using $PY"

# ─────────────────────────────────────────────────────────────────
# pipx
# ─────────────────────────────────────────────────────────────────

install_pipx() {
  # macOS — prefer Homebrew (avoids PEP 668 issues)
  if have_cmd brew && [ "$(uname -s)" = "Darwin" ]; then
    print_info "Installing pipx via Homebrew..."
    brew install pipx
    return 0
  fi

  # Linux — try to guide toward system package first
  if have_cmd apt-get; then
    die "\
pipx is required but not found.

  Install it with:

    sudo apt update && sudo apt install -y pipx python3-venv

  Then re-run this installer."

  elif have_cmd dnf; then
    die "\
pipx is required but not found.

  Install it with:

    sudo dnf install pipx

  Then re-run this installer."

  elif have_cmd pacman; then
    die "\
pipx is required but not found.

  Install it with:

    sudo pacman -S python-pipx

  Then re-run this installer."
  fi

  # Fallback: pip install --user
  print_info "Installing pipx via pip..."
  "$PY" -m pip install --user pipx 2>&1 || die "\
Failed to install pipx via pip.

  Try installing pipx manually:
    python3 -m pip install --user pipx
  or use your system package manager, then re-run."
}

# --- Ensure pipx is available ---

if ! have_cmd pipx; then
  install_pipx

  # After install, pipx may not be in PATH yet for this shell session.
  # Ensure pipx's bin dir is on PATH before we try to use it.
  if ! have_cmd pipx; then
    # refresh PATH from profile files as a best-effort
    export PATH="$HOME/.local/bin:$HOME/.local/pipx/bin:$PATH"
    hash -r 2>/dev/null || true
  fi

  if ! have_cmd pipx; then
    die "\
pipx was installed but is not yet in your PATH.

  Add pipx to your shell profile manually:

    echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc
    # or ~/.zshrc / ~/.config/fish/config.fish

  Then restart your terminal or run:

    export PATH=\"\$HOME/.local/bin:\$PATH\"

  and re-run this installer."

  fi
fi

# Ensure pipx's bin directories are on PATH for the rest of the script
"$PY" -m pipx ensurepath >/dev/null 2>&1 || true
export PATH="$HOME/.local/bin:$HOME/.local/pipx/bin:$PATH"
hash -r 2>/dev/null || true

print_ok "pipx ready"

# ─────────────────────────────────────────────────────────────────
# Install embed-log
# ─────────────────────────────────────────────────────────────────

# Detect local clone vs. remote install
INSTALL_SRC=""

# Check if BASH_SOURCE points to a real file (local clone)
if [ -n "${BASH_SOURCE[0]:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || true)"
  if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/pyproject.toml" ] && [ -d "$SCRIPT_DIR/backend" ]; then
    INSTALL_SRC="$SCRIPT_DIR"
  fi
fi

# Also check current directory (covers `cd repo && ./install.sh`)
if [ -z "$INSTALL_SRC" ] && [ -f pyproject.toml ] && [ -d backend ]; then
  INSTALL_SRC="$(pwd)"
fi

if [ -n "$INSTALL_SRC" ]; then
  print_info "Installing from local repository at ${INSTALL_SRC}..."
  PIPX_SRC="$INSTALL_SRC"
else
  print_info "Fetching embed-log from GitHub (${REPO})..."
  if have_cmd git; then
    PIPX_SRC="git+${REPO_URL}@${BRANCH}"
  else
    print_warn "git not found — downloading source archive instead."
    TMPDIR="$(mktemp -d)"
    ARCHIVE_URL="https://github.com/${REPO}/archive/${BRANCH}.tar.gz"
    print_info "Downloading ${ARCHIVE_URL}..."
    curl -fsSL "$ARCHIVE_URL" | tar xz -C "$TMPDIR" || die "\
Failed to download embed-log source from GitHub.

  Check your internet connection and try again."

    # GitHub archives extract to <repo>-<branch>/
    EXTRACTED="$(cd "$TMPDIR" && ls -d embed-log-* 2>/dev/null | head -1)"
    [ -n "$EXTRACTED" ] || die "Downloaded archive has unexpected structure."
    PIPX_SRC="${TMPDIR}/${EXTRACTED}"
  fi
fi

# Handle reinstall / upgrade
INSTALL_CMD=(pipx install --force "$PIPX_SRC")
print_info "Running: ${INSTALL_CMD[*]}"
"${INSTALL_CMD[@]}" || die "\
Failed to install embed-log via pipx.

  If you see version conflict errors, try:
    pipx uninstall embed-log
    pipx install ${PIPX_SRC}"

# Clean up temp dir if we used one
if [ -n "${TMPDIR:-}" ] && [ -d "$TMPDIR" ]; then
  rm -rf "$TMPDIR"
fi

# ─────────────────────────────────────────────────────────────────
# Done
# ─────────────────────────────────────────────────────────────────

echo ""
print_ok "embed-log installed!"
echo ""
echo "  Run from any directory:"
echo ""
echo "    embed-log --help"
echo ""
echo "  Quick start:"
echo ""
echo "    embed-log init"
echo "    embed-log run --config embed-log.yml"
echo ""
echo "  If the command is not found, open a new terminal (PATH refresh)."
