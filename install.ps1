<#
.SYNOPSIS
    embed-log installer for Windows (PowerShell)
.DESCRIPTION
    Installs embed-log globally via pipx. Requires Python >= 3.10.
    Run from PowerShell:
        iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
#>

$ErrorActionPreference = 'Stop'

$Repo = 'krezolekcoder/embed-log'
$Branch = 'main'
$RepoUrl = "https://github.com/$Repo.git"
$MinPy = [Version]'3.10'

function Write-Info { Write-Host "embed-log $args" -ForegroundColor Cyan }
function Write-OK { Write-Host "  ✓ $args" -ForegroundColor Green }
function Write-Warn { Write-Host "  ⚠ $args" -ForegroundColor Yellow }
function Die {
    Write-Host "`n  ✕ $args" -ForegroundColor Red
    exit 1
}

function Have-Cmd {
    param([string]$Cmd)
    return [bool](Get-Command $Cmd -ErrorAction SilentlyContinue)
}

function Invoke-Pipx {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)
    if (Have-Cmd 'pipx') {
        & pipx @Args
    } else {
        & $script:python -m pipx @Args
    }
}

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

if (-not (Have-Cmd 'pipx')) {
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
}

$userScripts = & $python -c "import os, site; print(os.path.join(site.USER_BASE, 'Scripts'))" 2>$null
if ($userScripts) {
    $env:Path = "$userScripts;$env:Path"
}
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + $env:Path
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + $env:Path

& $python -m pipx ensurepath 2>&1 | Out-Null
& $python -m pipx --version 2>$null | Out-Null
if (-not $?) {
    Die @"
pipx was installed but could not be started.

  Restart PowerShell and try again.
  If that still fails, run:
    python -m pip install --user pipx
    python -m pipx ensurepath
"@
}

Write-OK "pipx ready"

$installSrc = $null
$tmpRoot = $null
$localRoot = $PSScriptRoot
if ($localRoot -and (Test-Path (Join-Path $localRoot 'pyproject.toml')) -and (Test-Path (Join-Path $localRoot 'backend'))) {
    Write-Info "Installing from local repository at $localRoot..."
    $installSrc = $localRoot
} elseif (Have-Cmd 'git') {
    Write-Info "Installing embed-log from GitHub ($Repo)..."
    $installSrc = "git+$RepoUrl@$Branch"
} else {
    Write-Warn "git not found — downloading source archive instead."
    $tmpRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("embed-log-" + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $tmpRoot | Out-Null
    $archivePath = Join-Path $tmpRoot 'embed-log.tar.gz'
    $archiveUrl = "https://github.com/$Repo/archive/$Branch.tar.gz"
    Write-Info "Downloading $archiveUrl..."
    Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath
    & $python -c "import tarfile; tarfile.open(r'''$archivePath''', 'r:gz').extractall(r'''$tmpRoot''')"
    if (-not $?) {
        Die "Failed to extract embed-log source archive."
    }
    $srcDir = Get-ChildItem -Path $tmpRoot -Directory | Select-Object -First 1
    if (-not $srcDir) {
        Die "Downloaded archive has unexpected structure."
    }
    $installSrc = $srcDir.FullName
}

try {
    Invoke-Pipx install --force $installSrc 2>&1
    if (-not $?) {
        Die @"
Failed to install embed-log via pipx.

  If you see version conflict errors, try:
    pipx uninstall embed-log
    pipx install --force $installSrc
"@
    }
} finally {
    if ($tmpRoot -and (Test-Path $tmpRoot)) {
        Remove-Item -Recurse -Force $tmpRoot
    }
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
