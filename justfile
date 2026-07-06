# Project task runner for embed-log.
# Run `just --list` to see the maintained command surface.

set dotenv-load := true

cargo := "cargo"
node_package_dir := "tests-ui"
config := "demo.yml"
frontend_dir := "frontend"

# Show available recipes.
default:
    @just --list

# Build the embed-log CLI release binary.
build:
    {{cargo}} build --locked --release --package embed-log-cli --bin embed-log

# Build the Tauri desktop app release binary.
build-desktop:
    {{cargo}} build --locked --release --package embed-log-tauri --bin embed-log-tauri

# Build and install the embed-log CLI from target/release.
# Defaults to /usr/local/bin. Override: just install /opt/homebrew/bin
install install_dir="/usr/local/bin": build
    #!/usr/bin/env sh
    set -eu
    src="target/release/embed-log"
    install_dir="{{install_dir}}"
    dest="$install_dir/embed-log"
    if [ ! -d "$install_dir" ]; then
      if [ "$(id -u)" -eq 0 ]; then
        mkdir -p "$install_dir"
      else
        sudo mkdir -p "$install_dir"
      fi
    fi
    if [ -w "$install_dir" ]; then
      install -m 755 "$src" "$dest"
    else
      sudo install -m 755 "$src" "$dest"
    fi
    echo "Installed embed-log to $dest"
    "$dest" --version

# Remove the installed embed-log CLI.
# Defaults to /usr/local/bin. Override: just uninstall /opt/homebrew/bin
uninstall install_dir="/usr/local/bin":
    #!/usr/bin/env sh
    set -eu
    dest="{{install_dir}}/embed-log"
    if [ ! -e "$dest" ]; then
      echo "Nothing to uninstall at $dest"
      exit 0
    fi
    if [ -w "{{install_dir}}" ]; then
      rm -f "$dest"
    else
      sudo rm -f "$dest"
    fi
    echo "Removed $dest"

# Run embed-log in one of four modes: web, headless, tui, desktop.
# Examples: just run / just run headless demo.yml / just run tui embed-log.yml / just run desktop embed-log.yml
run mode="web" cfg=config:
    #!/usr/bin/env sh
    set -eu
    case "{{mode}}" in
      web)
        exec {{cargo}} run --package embed-log-cli --bin embed-log -- run --config {{cfg}} --frontend-dir {{frontend_dir}}
        ;;
      headless)
        exec {{cargo}} run --package embed-log-cli --bin embed-log -- run --config {{cfg}} --frontend-dir {{frontend_dir}} --no-open-browser
        ;;
      tui)
        exec {{cargo}} run --package embed-log-cli --bin embed-log -- run --tui --config {{cfg}} --no-open-browser
        ;;
      desktop)
        exec {{cargo}} run --package embed-log-tauri --bin embed-log-tauri -- --config {{cfg}}
        ;;
      *)
        echo "unknown run mode: {{mode}} (expected: web, headless, tui, desktop)" >&2
        exit 1
        ;;
    esac

# Run demo traffic in one of four modes: web, headless, tui, desktop.
# Examples: just demo / just demo headless / just demo tui / just demo desktop
demo mode="web":
    #!/usr/bin/env sh
    set -eu
    case "{{mode}}" in
      web)
        exec {{cargo}} run --package embed-log-cli --bin embed-log -- demo --config {{config}} --frontend-dir {{frontend_dir}}
        ;;
      headless)
        exec {{cargo}} run --package embed-log-cli --bin embed-log -- demo --config {{config}} --frontend-dir {{frontend_dir}} --no-open-browser
        ;;
      tui)
        exec {{cargo}} run --package embed-log-cli --bin embed-log -- demo --tui --config {{config}} --no-open-browser
        ;;
      desktop)
        exec env EMBED_LOG_DEMO_TRAFFIC=1 {{cargo}} run --package embed-log-tauri --bin embed-log-tauri -- --config {{config}}
        ;;
      *)
        echo "unknown demo mode: {{mode}} (expected: web, headless, tui, desktop)" >&2
        exit 1
        ;;
    esac

# Run tests by scope.
# Scopes: rust, ui-setup, ui-unit, ui, regression, all.
# Examples: just test / just test ui-setup / just test ui / just test all
test scope="rust":
    #!/usr/bin/env sh
    set -eu
    case "{{scope}}" in
      rust)
        exec {{cargo}} test --workspace
        ;;
      ui-setup)
        npm --prefix {{node_package_dir}} ci
        exec npm --prefix {{node_package_dir}} run install-browsers
        ;;
      ui-unit)
        exec npm --prefix {{node_package_dir}} run test:unit
        ;;
      ui)
        exec npm --prefix {{node_package_dir}} run test:e2e
        ;;
      regression)
        exec npm --prefix {{node_package_dir}} run test:regression
        ;;
      all)
        {{cargo}} test --workspace
        npm --prefix {{node_package_dir}} run test:unit
        exec npm --prefix {{node_package_dir}} run test:e2e
        ;;
      *)
        echo "unknown test scope: {{scope}} (expected: rust, ui-setup, ui-unit, ui, regression, all)" >&2
        exit 1
        ;;
    esac

# Run the standard local verification set.
verify:
    {{cargo}} fmt --all -- --check
    {{cargo}} check --workspace
    {{cargo}} clippy --workspace --all-targets -- -D warnings
    {{cargo}} test --workspace
    npm --prefix {{node_package_dir}} run test:unit

# Remove build outputs managed by Cargo.
clean:
    {{cargo}} clean
