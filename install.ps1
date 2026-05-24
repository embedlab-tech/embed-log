<#
.SYNOPSIS
    embed-log installer for Windows (PowerShell)
.DESCRIPTION
    Installs embed-log globally via pipx.  Requires Python >= 3.10.
    Run from PowerShell:
        iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
#>

$ErrorActionPreference = 'Stop'

# ── Config ───────────────────────────────────────────────────────
$Repo   = 'krezolekcoder/embed-log'
$Branch = 'main'
$RepoUrl = "https://github.com/$Repo.git"
$MinPy  = [Version]'3.10'

# ── Helpers ──────────────────────────────────────────────────────
function Write-Info  { Write-Host "embed-log $args" -ForegroundColor Cyan }
function Write-OK   { Write-Host "  ✓ $args" -ForegroundColor Green }
function Write-Warn { Write-Host "  ⚠ $args" -ForegroundColor Yellow }
function Die {
    Write-Host "`n  ✕ $args" -ForegroundColor Red
    exit 1
}

function Have-Cmd {
    param([string]$Cmd)
    return [bool](Get-Command $Cmd -ErrorAction SilentlyContinue)
}

# ── Python version check ─────────────────────────────────────────
Write-Info "Checking Python..."

$python = $null
foreach ($c in @('py', 'python3', 'python')) {
    if (Have-Cmd $c) {
        $python = $c
        break
    }
}

if (-not $python) {
    Die @"
Python not found (need >= $MinPy).

  Install Python 3.10 or later from:
    https://python.org

  Make sure to check "Add Python to PATH" during installation,
  then open a new PowerShell window and try again.
"@
}

$pyVerStr = & $python -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')" 2>$null
if (-not $pyVerStr) {
    Die "Failed to run Python interpreter ($python)."
}

$pyVer = [Version]::new($pyVerStr)
if ($pyVer -lt $MinPy) {
    Die @"
Python $pyVerStr is too old — version $MinPy or later is required.

  Upgrade Python from https://python.org and try again.
"@
}

Write-OK "Python $pyVerStr — using $python"

# ── pipx ─────────────────────────────────────────────────────────
if (-not (Have-Cmd pipx)) {
    Write-Info "pipx not found — installing via pip..."
    & $python -m pip install --user pipx 2>&1 | Out-Null
    if (-not $?) {
        Die @"
Failed to install pipx via pip.

  Try:
    python -m pip install --user pipx
  or:
    pip install --user pipx

  Then restart PowerShell and try again.
"@
    }

    # Refresh PATH to include pipx
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + $env:Path
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + $env:Path

    if (-not (Have-Cmd pipx)) {
        Die @'
pipx was installed but is not in your PATH.

  Add it manually:
    [Environment]::SetEnvironmentVariable('Path',
      $env:Path + ';' + $env:USERPROFILE + '\.local\bin',
      'User')

  Then restart PowerShell and re-run this script.
'@
    }
}

Write-OK "pipx ready"

# ── Install embed-log ────────────────────────────────────────────
Write-Info "Installing embed-log from GitHub ($Repo)..."

$pipxArgs = @('install', '--force', "git+${RepoUrl}@${Branch}")
& pipx @pipxArgs 2>&1
if (-not $?) {
    Die @"
Failed to install embed-log via pipx.

  If you see version conflict errors, try:
    pipx uninstall embed-log
    pipx install "git+${RepoUrl}"
"@
}

Write-OK "embed-log installed!"
Write-Host ""
Write-Host "  Run from any terminal:"
Write-Host ""
Write-Host "    embed-log --help"
Write-Host ""
Write-Host "  Quick start:"
Write-Host ""
Write-Host "    embed-log init"
Write-Host "    embed-log run --config embed-log.yml"
Write-Host ""
Write-Host "  If the command is not found, open a new terminal (PATH refresh)."
