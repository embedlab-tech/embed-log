# Project task runner for embed-log-rs.
# Run `just --list` to see available recipes.

set dotenv-load := true

cargo := "cargo"
node_package_dir := "tests-ui"
config := "demo.yml"
frontend_dir := "frontend"
log_dir := "logs"

# Show available recipes.
default:
    @just --list

# Build all Rust crates.
build:
    {{cargo}} build --workspace

# Build release binaries.
build-release:
    {{cargo}} build --workspace --release

# Build and package the CLI for an explicit target triple. Example: just package-cli x86_64-unknown-linux-gnu
package-cli target:
    {{cargo}} build --locked --release --package embed-log-cli --bin embed-log
    ./scripts/package-cli.sh {{target}}

# Build and package the CLI for the current macOS/Linux host.
package-cli-current:
    #!/usr/bin/env sh
    set -eu
    case "$(uname -s)" in
      Darwin) os="apple-darwin" ;;
      Linux) os="unknown-linux-gnu" ;;
      *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
    esac
    case "$(uname -m)" in
      x86_64|amd64) arch="x86_64" ;;
      arm64|aarch64) arch="aarch64" ;;
      *) echo "unsupported CPU architecture: $(uname -m)" >&2; exit 1 ;;
    esac
    target="$arch-$os"
    {{cargo}} build --locked --release --package embed-log-cli --bin embed-log
    ./scripts/package-cli.sh "$target"

# Build and package both macOS CLI artifacts from an Apple Silicon Mac.
package-cli-macos:
    {{cargo}} build --locked --release --package embed-log-cli --bin embed-log
    ./scripts/package-cli.sh aarch64-apple-darwin
    rustup target add x86_64-apple-darwin
    {{cargo}} build --locked --release --package embed-log-cli --bin embed-log --target x86_64-apple-darwin
    BIN_PATH=target/x86_64-apple-darwin/release/embed-log ./scripts/package-cli.sh x86_64-apple-darwin

# Build and package the Linux x64 CLI artifact.
package-cli-linux:
    {{cargo}} build --locked --release --package embed-log-cli --bin embed-log
    ./scripts/package-cli.sh x86_64-unknown-linux-gnu

# Build and package the Windows x64 CLI artifact. Run from PowerShell on the Windows runner.
package-cli-windows:
    {{cargo}} build --locked --release --package embed-log-cli --bin embed-log
    pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/package-cli.ps1 -Target x86_64-pc-windows-msvc

# Validate release helper scripts.
release-check:
    #!/usr/bin/env sh
    set -eu
    sh -n install.sh
    sh -n scripts/package-cli.sh
    if command -v pwsh >/dev/null 2>&1; then
      pwsh -NoProfile -Command '$ErrorActionPreference = "Stop"; [scriptblock]::Create((Get-Content -Raw "install.ps1")) | Out-Null; [scriptblock]::Create((Get-Content -Raw "scripts/package-cli.ps1")) | Out-Null'
    else
      echo "pwsh not found; skipping PowerShell syntax check"
    fi

# Create and push a release tag. Usage: just release-tag v0.1.0
release-tag version:
    git tag {{version}}
    git push origin {{version}}

# Type-check all Rust crates.
check:
    {{cargo}} check --workspace

# Run Rust formatter.
fmt:
    {{cargo}} fmt --all

# Check Rust formatting without modifying files.
fmt-check:
    {{cargo}} fmt --all -- --check

# Run Clippy with warnings denied.
clippy:
    {{cargo}} clippy --workspace --all-targets -- -D warnings

# Run Rust unit/integration tests.
test:
    {{cargo}} test --workspace

# Run the standard local verification set.
verify: fmt-check check clippy test ui-unit

# Run the CLI with a config file. Override: just run path/to/config.yml
run cfg=config:
    {{cargo}} build --package embed-log-cli --bin embed-log
    exec ./target/debug/embed-log run --config {{cfg}} --frontend-dir {{frontend_dir}}

# Run the CLI without opening a browser. Override: just run-headless path/to/config.yml
run-headless cfg=config:
    {{cargo}} build --package embed-log-cli --bin embed-log
    exec ./target/debug/embed-log run --config {{cfg}} --frontend-dir {{frontend_dir}} --no-open-browser

# Run the web demo server with generated demo traffic.
demo: demo-web

# Run the web demo server with generated demo traffic.
demo-web:
    {{cargo}} build --package embed-log-cli --bin embed-log
    exec ./target/debug/embed-log demo --config {{config}} --frontend-dir {{frontend_dir}}

# Run the web demo server with generated demo traffic and no browser.
demo-headless:
    {{cargo}} build --package embed-log-cli --bin embed-log
    exec ./target/debug/embed-log demo --config {{config}} --frontend-dir {{frontend_dir}} --no-open-browser

# Run the desktop/Tauri demo with the same generated demo traffic.
demo-desktop:
    {{cargo}} build --package embed-log-tauri --bin embed-log-tauri
    EMBED_LOG_DEMO_TRAFFIC=1 exec ./target/debug/embed-log-tauri --config {{config}}


# Build the terminal UI (ratatui) crate.
build-tui:
    {{cargo}} build --package embed-log-tui --bin embed-log-tui

# Run the terminal UI demo with generated demo traffic (ratatui).
#
# Single command: `embed-log demo --tui` starts the server + demo traffic
# in-process and connects the TUI. Quit with `q` or Ctrl+C.
demo-tui:
    {{cargo}} build --package embed-log-cli --bin embed-log
    exec ./target/debug/embed-log demo --tui --config demo.yml --no-open-browser

# Run the terminal UI against a config (server + TUI in one process).
run-tui cfg=config:
    {{cargo}} build --package embed-log-cli --bin embed-log
    exec ./target/debug/embed-log run --tui --config {{cfg}} --no-open-browser

# Run terminal UI unit tests.
tui-unit:
    {{cargo}} test --package embed-log-tui

# Run first-run onboarding. Default: web. Override: just onboarding desktop
onboarding target="web":
    @just onboarding-{{target}}

# Run first-run onboarding in the browser (CLI). Override: just onboarding-web /path/to/cfg.yml
onboarding-web cfg="/tmp/embed-log-onboarding-web.yml":
    rm -f {{cfg}}
    {{cargo}} build --package embed-log-cli --bin embed-log
    env -u EMBED_LOG_CONFIG_YML_PATH ./target/debug/embed-log onboard --config {{cfg}} --frontend-dir {{frontend_dir}}

# Run first-run onboarding in the Tauri desktop app. Override: just onboarding-desktop /path/to/cfg.yml
onboarding-desktop cfg="/tmp/embed-log-onboarding-desktop.yml":
    rm -f {{cfg}}
    {{cargo}} build --package embed-log-tauri --bin embed-log-tauri
    env -u EMBED_LOG_CONFIG_YML_PATH ./target/debug/embed-log-tauri --config {{cfg}}

# Generate a sample embed-log.yml config.
init out="embed-log.yml":
    {{cargo}} run --package embed-log-cli --bin embed-log -- init --output {{out}}

# Show runtime diagnostics. Override: just doctor path/to/config.yml
doctor cfg=config:
    {{cargo}} run --package embed-log-cli --bin embed-log -- doctor --config {{cfg}}

# List detected serial ports.
ports:
    {{cargo}} run --package embed-log-cli --bin embed-log -- ports

# List recorded sessions. Override: just sessions logs
sessions dir=log_dir:
    {{cargo}} run --package embed-log-cli --bin embed-log -- sessions list --dir {{dir}}

# Export one session to HTML. Usage: just export-session SESSION_ID [logs] [output.html]
export-session session_id dir=log_dir output="session.html":
    {{cargo}} run --package embed-log-cli --bin embed-log -- sessions export {{session_id}} --dir {{dir}} --output {{output}} --format html

# Merge raw logs into static HTML. Usage: just merge 'Main' PANE path/to/log [merged.html]
merge tab pane file output="merged.html":
    {{cargo}} run --package embed-log-cli --bin embed-log -- merge --tab {{tab}} {{pane}} {{file}} --output {{output}}

# Install UI test dependencies.
ui-install:
    npm --prefix {{node_package_dir}} install

# Install Playwright Chromium for UI tests.
ui-install-browsers:
    npm --prefix {{node_package_dir}} run install-browsers

# Run frontend unit tests.
ui-unit:
    npm --prefix {{node_package_dir}} run test:unit

# Run regular Playwright UI tests.
ui-e2e:
    npm --prefix {{node_package_dir}} run test:e2e

# Run all UI regression Playwright tests in one Playwright invocation.
ui-regression:
    npm --prefix {{node_package_dir}} run test:regression

# Show UI regression Playwright test categories.
ui-regression-list:
    npm --prefix {{node_package_dir}} run test:regression:list

# Run UI regression Playwright tests category-by-category.
ui-regression-categorized:
    npm --prefix {{node_package_dir}} run test:regression:categorized

# Run regression smoke tests (core live UI smoke, demo traffic, stats, timestamp toggle).
ui-regression-smoke:
    npm --prefix {{node_package_dir}} run test:regression:smoke

# Run regression data/source/plugin tests (CBOR, network capture, CoAP plugin, plugin isolation).
ui-regression-data:
    npm --prefix {{node_package_dir}} run test:regression:data

# Run regression interaction tests (clipboard, drag, filters, layout sync, selection scopes).
ui-regression-interaction:
    npm --prefix {{node_package_dir}} run test:regression:interaction

# Run regression event-detection tests.
ui-regression-events:
    npm --prefix {{node_package_dir}} run test:regression:events

# Run regression session/export/replay tests.
ui-regression-sessions:
    npm --prefix {{node_package_dir}} run test:regression:sessions

# Run all UI tests defined by tests-ui/package.json.
ui-all:
    npm --prefix {{node_package_dir}} run test:all

# Remove build outputs managed by Cargo.
clean:
    {{cargo}} clean
