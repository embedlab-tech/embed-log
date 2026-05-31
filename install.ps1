<#
.SYNOPSIS
    embed-log installer for Windows (PowerShell 7+)
.DESCRIPTION
    Installs embed-log globally via pipx. Requires Python >= 3.10 and PowerShell 7+.
    Run from PowerShell:
        iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
#>

$ErrorActionPreference = 'Stop'

$Repo = 'krezolekcoder/embed-log'
$Branch = 'main'
$RepoUrl = "https://github.com/$Repo.git"
$MinPy = [Version]'3.10'
$InstallRefType = if ($env:EMBED_LOG_REF_TYPE) { $env:EMBED_LOG_REF_TYPE } else { 'branch' }
$InstallRef = if ($env:EMBED_LOG_REF) { $env:EMBED_LOG_REF } else { $Branch }
$OverrideRepo = if ($env:EMBED_LOG_REPO) { $env:EMBED_LOG_REPO } else { $Repo }
$OverrideRepoUrl = if ($env:EMBED_LOG_REPO_URL) { $env:EMBED_LOG_REPO_URL } else { "https://github.com/$OverrideRepo.git" }

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

function Write-VersionFile {
    param([string]$Dir, [string]$Commit)
    $versionFile = Join-Path $Dir 'backend\_version.py'
    @"
# Auto-generated. Do not edit manually.
# Install scripts populate __commit__ before pipx install.
__version__ = "1.0.1"
__commit__ = "$Commit"
"@ | Set-Content -Path $versionFile -Encoding UTF8
}
function Write-InstallSourceFile {
    param(
        [string]$Dir,
        [string]$SourceKind,
        [string]$Repo,
        [string]$RepoUrl,
        [string]$RefType,
        [string]$Ref,
        [string]$LocalPath
    )
    $sourceFile = Join-Path $Dir 'backend\_install_source.py'
    @"
# Auto-generated. Install scripts populate these before pipx install.
__source_kind__ = "$SourceKind"
__repo__ = "$Repo"
__repo_url__ = "$RepoUrl"
__ref_type__ = "$RefType"
__ref__ = "$Ref"
__local_path__ = "$LocalPath"
"@ | Set-Content -Path $sourceFile -Encoding UTF8
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
function Resolve-RequestedRef {
    if ($InstallRefType -ne 'release') {
        return $InstallRef
    }
    if ($InstallRef -ne 'latest') {
        return $InstallRef
    }
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$OverrideRepo/releases/latest"
    if (-not $release.tag_name) {
        Die "Failed to resolve latest release tag."
    }
    return $release.tag_name
}


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
$ResolvedInstallRef = Resolve-RequestedRef
Write-OK "Install ref $InstallRefType`:$InstallRef -> $ResolvedInstallRef"

$installSrc = $null
$tmpRoot = $null
$localRoot = $PSScriptRoot
if ($localRoot -and (Test-Path (Join-Path $localRoot 'pyproject.toml')) -and (Test-Path (Join-Path $localRoot 'backend'))) {
    Write-Info "Installing from local repository at $localRoot..."
    $installSrc = $localRoot
    if (Have-Cmd 'git') {
        $sha = & git -C $localRoot rev-parse --short HEAD 2>$null
        if ($sha) {
            Write-VersionFile -Dir $localRoot -Commit $sha
        }
    }
    Write-InstallSourceFile -Dir $localRoot -SourceKind 'local' -Repo $OverrideRepo -RepoUrl $OverrideRepoUrl -RefType $InstallRefType -Ref $InstallRef -LocalPath $localRoot
} elseif (Have-Cmd 'git') {
    Write-Info "Installing embed-log from GitHub ($OverrideRepo)..."
    if (-not $env:USERPROFILE) {
        Die "USERPROFILE is not set."
    }
    $cacheBase = Join-Path $env:USERPROFILE '.cache\embed-log'
    $cacheRoot = Join-Path $cacheBase 'src'
    $null = New-Item -ItemType Directory -Force -Path $cacheBase
    if (Test-Path $cacheRoot) {
        Remove-Item -Recurse -Force $cacheRoot
    }
    & git init $cacheRoot 2>&1 | Out-Null
    if (-not $?) {
        Die "Failed to prepare embed-log cache directory."
    }
    & git -C $cacheRoot remote add origin $OverrideRepoUrl 2>&1 | Out-Null
    if (-not $?) {
        Die "Failed to configure embed-log repository origin."
    }
    & git -C $cacheRoot fetch --depth=1 origin $ResolvedInstallRef 2>&1 | Out-Null
    if (-not $?) {
        Die "Failed to fetch embed-log ref '$ResolvedInstallRef'."
    }
    & git -C $cacheRoot checkout --detach FETCH_HEAD 2>&1 | Out-Null
    if (-not $?) {
        Die "Failed to checkout embed-log ref '$ResolvedInstallRef'."
    }
    $installSrc = $cacheRoot
    $sha = & git -C $cacheRoot rev-parse --short HEAD 2>$null
    if ($sha) {
        Write-VersionFile -Dir $cacheRoot -Commit $sha
    }
    Write-InstallSourceFile -Dir $cacheRoot -SourceKind 'git' -Repo $OverrideRepo -RepoUrl $OverrideRepoUrl -RefType $InstallRefType -Ref $InstallRef -LocalPath ''
} else {
    Write-Warn "git not found — downloading source archive instead."
    $tmpRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("embed-log-" + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $tmpRoot | Out-Null
    $archivePath = Join-Path $tmpRoot 'embed-log.tar.gz'
    $archiveUrl = "https://github.com/$OverrideRepo/archive/$ResolvedInstallRef.tar.gz"
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
    Write-VersionFile -Dir $srcDir.FullName -Commit 'archive'
    Write-InstallSourceFile -Dir $srcDir.FullName -SourceKind 'archive' -Repo $OverrideRepo -RepoUrl $OverrideRepoUrl -RefType $InstallRefType -Ref $InstallRef -LocalPath ''
}

try {
    Invoke-Pipx uninstall embed-log 2>&1 | Out-Null
    if ($?) {
        Write-Info "Removed existing embed-log installation."
    }

    Invoke-Pipx install $installSrc 2>&1
    if (-not $?) {
        Die @"
Failed to install embed-log via pipx.

  If installation fails, try:
    pipx uninstall embed-log
    pipx install $installSrc
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
Write-Host "    embed-log create-config"
Write-Host "    embed-log run --config embed-log.yml"
Write-Host ""
Write-Host "  If the command is not found, open a new terminal (PATH refresh)."
