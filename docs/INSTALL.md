# Installation

`embed-log` runs as a configurable multi-source backend (UART/UDP + TCP inject/TX + session artifacts) with a browser UI whose tabs/panes are defined by backend config.

---

## Requirements

- Python **3.10+**
- A modern browser (Chrome/Firefox/Safari/Edge)

---

## Quick install (recommended)

One command, no clone needed. Installs `embed-log` globally via `pipx` so it's available from any directory.
On macOS this is typically one step. On Linux, if `pipx` is not already installed, the script prints the exact package-manager command to install it and you then rerun the installer.

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
```

**Windows (PowerShell):**

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass; iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
```

After install:

```bash
embed-log init
embed-log run --config embed-log.yml
```

The default UI is at `http://127.0.0.1:8080/`.

> **Note:** If you get "command not found", open a new terminal to refresh your PATH.
> Or run `export PATH="$HOME/.local/bin:$PATH"` in the current shell.

---

## What the installer does

The install script (`install.sh` / `install.ps1`):

1. Checks for **Python 3.10+** (with clear guidance if missing)
2. Installs or bootstraps **pipx** when possible, and otherwise prints the exact package-manager command needed
3. Runs `pipx install` to download and install `embed-log` from GitHub into an isolated environment
4. Falls back to downloading a source tarball from GitHub when `git` is unavailable

No venv activation is needed. The pipx install is isolated and can be uninstalled with `pipx uninstall embed-log`.

---

## Alternative: install from a local clone

If you already have the repository cloned, run the installer from the repo root:

```bash
./install.sh
```

The script auto-detects the local `pyproject.toml` and installs from the local source instead of fetching from GitHub.

---

## Developer setup (project venv)

For development, testing, or running directly from source:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
pip install -e .
```

Then use:

```bash
embed-log init
embed-log validate --config embed-log.yml
embed-log run --config embed-log.yml
```

If `embed-log` is not on your PATH in the venv, use:

```bash
python3 backend/server.py init
python3 backend/server.py validate --config embed-log.yml
python3 backend/server.py run --config embed-log.yml
```

Run the bundled demo:

```bash
./run_demo.sh
# optional: avoid auto-opening browser
./run_demo.sh --no-browser
```

---

## Reinstall after local code changes

```bash
pipx reinstall embed-log          # reinstall from current pipx cache
# or
pipx install --force .            # reinstall from local directory
# or
python3 -m build
pipx install --force dist/embed_log-*.whl
```

## Uninstall

```bash
pipx uninstall embed-log
```

## Unit tests

```bash
# from project venv:
python3 -m unittest discover -s tests -v
```

---

## Legacy: run directly from source

Still supported from project root:

```bash
python3 backend/server.py run --config examples/embed-log.yml
```

But end users should prefer the global `embed-log` CLI installed via the quick install method above.
