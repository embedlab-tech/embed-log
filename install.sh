#!/usr/bin/env bash
#
# embed-log installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
#   ./install.sh                          install latest release
#   ./install.sh --main                   install latest commit from main branch
#   ./install.sh --develop                install latest commit from develop branch
#
# Installs embed-log globally via pipx.  Requires Python >= 3.10.
# Works on macOS, Linux, and (via WSL/Git-Bash) Windows.
#
# Environment overrides (take precedence over CLI args):
#   EMBED_LOG_REF_TYPE  release|branch|tag|commit  (default: release)
#   EMBED_LOG_REF       ref value                   (default: latest)
#   EMBED_LOG_REPO      fork/other repo             (default: krezolekcoder/embed-log)

set -euo pipefail

# ─────────────────────────────────────────────────────────────────
# Config
# ─────────────────────────────────────────────────────────────────

INSTALL_TMPDIR=""
REPO="krezolekcoder/embed-log"
REPO_URL="https://github.com/${REPO}.git"
MIN_PY="3.10"
INSTALL_REF_TYPE="${EMBED_LOG_REF_TYPE:-release}"
INSTALL_REF="${EMBED_LOG_REF:-latest}"
OVERRIDE_REPO="${EMBED_LOG_REPO:-$REPO}"
OVERRIDE_REPO_URL="${EMBED_LOG_REPO_URL:-https://github.com/${OVERRIDE_REPO}.git}"

INSTALLER_VERSION="1.0.0"
# If a --<branch> argument is passed, treat it as a branch install.
# Environment variables take precedence, so only parse if they're unset.
if [ $# -ge 1 ] && [ -z "${EMBED_LOG_REF_TYPE+x}" ] && [ -z "${EMBED_LOG_REF+x}" ]; then
  case "$1" in
    --*)
      INSTALL_REF_TYPE="branch"
      INSTALL_REF="${1#--}"
      shift
      ;;
  esac
fi

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

_write_version() {
  local dir="$1" sha="$2"
  cat > "$dir/backend/_version.py" <<VERSION_EOF
# Auto-generated. Do not edit manually.
# Install scripts populate __commit__ before pipx install.
__version__ = "1.0.1"
__commit__ = "$sha"
VERSION_EOF
}
_write_install_source() {
  local dir="$1" source_kind="$2" repo="$3" repo_url="$4" ref_type="$5" ref="$6" local_path="$7"
  cat > "$dir/backend/_install_source.py" <<SOURCE_EOF
# Auto-generated. Install scripts populate these before pipx install.
__source_kind__ = "$source_kind"
__repo__ = "$repo"
__repo_url__ = "$repo_url"
__ref_type__ = "$ref_type"
__ref__ = "$ref"
__local_path__ = "$local_path"
SOURCE_EOF
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
_resolve_requested_ref() {
  case "$INSTALL_REF_TYPE" in
    release)
      [ "$INSTALL_REF" = "latest" ] || {
        echo "$INSTALL_REF"
        return 0
      }
      have_cmd curl || die "curl is required to resolve the latest release."
      curl -fsSL "https://api.github.com/repos/${OVERRIDE_REPO}/releases/latest" | \
        "$PY" -c 'import json,sys; data=json.load(sys.stdin); tag=data.get("tag_name"); sys.exit(1) if not tag else None; print(tag)'
      ;;
    *)
      echo "$INSTALL_REF"
      ;;
  esac
}
RESOLVED_INSTALL_REF="$(_resolve_requested_ref)" || die "Failed to resolve install ref."
print_ok "Install ref ${INSTALL_REF_TYPE}:${INSTALL_REF} -> ${RESOLVED_INSTALL_REF}"


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


if [ -n "$INSTALL_SRC" ]; then
  print_info "Installing from local repository at ${INSTALL_SRC}..."
  PIPX_SRC="$INSTALL_SRC"
  _commit=""
  if have_cmd git && git -C "$INSTALL_SRC" rev-parse --git-dir >/dev/null 2>&1; then
    _commit="$(git -C "$INSTALL_SRC" rev-parse --short HEAD 2>/dev/null || true)"
  fi
  if [ -n "$_commit" ]; then
    _write_version "$PIPX_SRC" "$_commit"
  fi
  _write_install_source "$PIPX_SRC" "local" "$OVERRIDE_REPO" "$OVERRIDE_REPO_URL" "$INSTALL_REF_TYPE" "$INSTALL_REF" "$INSTALL_SRC"
else
  print_info "Fetching embed-log from GitHub (${OVERRIDE_REPO})..."
  if have_cmd git; then
    [ -n "${HOME:-}" ] || die "HOME is not set."
    CACHE_BASE="$HOME/.cache/embed-log"
    CACHE_SRC="$CACHE_BASE/src"
    [ -e "$CACHE_SRC" ] && rm -rf "$CACHE_SRC"
    mkdir -p "$CACHE_BASE"
    git init "$CACHE_SRC" >/dev/null 2>&1 || die "Failed to prepare embed-log cache directory."
    git -C "$CACHE_SRC" remote add origin "$OVERRIDE_REPO_URL" >/dev/null 2>&1 || die "Failed to configure embed-log repository origin."
    git -C "$CACHE_SRC" fetch --depth=1 origin "$RESOLVED_INSTALL_REF" >/dev/null 2>&1 || die "\
Failed to fetch embed-log ref '${RESOLVED_INSTALL_REF}'.

  Check that the ref exists and try again."
    git -C "$CACHE_SRC" checkout --detach FETCH_HEAD >/dev/null 2>&1 || die "Failed to checkout embed-log ref '${RESOLVED_INSTALL_REF}'."

    PIPX_SRC="$CACHE_SRC"
    _commit="$(git -C "$PIPX_SRC" rev-parse --short HEAD 2>/dev/null || true)"
    if [ -n "$_commit" ]; then
      _write_version "$PIPX_SRC" "$_commit"
    fi
    _write_install_source "$PIPX_SRC" "git" "$OVERRIDE_REPO" "$OVERRIDE_REPO_URL" "$INSTALL_REF_TYPE" "$INSTALL_REF" ""
  else
    print_warn "git not found — downloading source archive instead."
    INSTALL_TMPDIR="$(mktemp -d)"
    ARCHIVE_URL="https://github.com/${OVERRIDE_REPO}/archive/${RESOLVED_INSTALL_REF}.tar.gz"
    print_info "Downloading ${ARCHIVE_URL}..."
    curl -fsSL "$ARCHIVE_URL" | tar xz -C "$INSTALL_TMPDIR" || die "\
Failed to download embed-log source from GitHub.

  Check your internet connection and try again."

    EXTRACTED="$(cd "$INSTALL_TMPDIR" && ls -d * 2>/dev/null | head -1)"
    [ -n "$EXTRACTED" ] || die "Downloaded archive has unexpected structure."
    PIPX_SRC="${INSTALL_TMPDIR}/${EXTRACTED}"
    _write_version "$PIPX_SRC" "archive"
    _write_install_source "$PIPX_SRC" "archive" "$OVERRIDE_REPO" "$OVERRIDE_REPO_URL" "$INSTALL_REF_TYPE" "$INSTALL_REF" ""
  fi
fi


# Replace an existing pipx install cleanly before reinstalling.
if pipx uninstall embed-log >/dev/null 2>&1; then
  print_info "Removed existing embed-log installation."
fi

INSTALL_CMD=(pipx install "$PIPX_SRC")
print_info "Running: ${INSTALL_CMD[*]}"
"${INSTALL_CMD[@]}" || die "\
Failed to install embed-log via pipx.

  If installation fails, try:
    pipx uninstall embed-log
    pipx install ${PIPX_SRC}"

# Clean up temp dir if we created one
if [ -n "$INSTALL_TMPDIR" ] && [ -d "$INSTALL_TMPDIR" ]; then
  rm -rf "$INSTALL_TMPDIR"
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
echo "    embed-log create-config"
echo "    embed-log run --config embed-log.yml"
echo ""
echo "  If the command is not found, open a new terminal (PATH refresh)."
