<#
.SYNOPSIS
    embed-log uninstaller for Windows (PowerShell 7+)
.DESCRIPTION
    Uninstalls embed-log from a pipx-managed global install. Requires PowerShell 7+.
    Run from PowerShell:
        iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/uninstall.ps1'))
#>

$ErrorActionPreference = 'Stop'

function Write-Info { Write-Host "embed-log $args" -ForegroundColor Cyan }
function Write-OK { Write-Host "  ✓ $args" -ForegroundColor Green }
function Die {
    Write-Host "`n  ✕ $args" -ForegroundColor Red
    exit 1
}

function Have-Cmd {
    param([string]$Cmd)
    return [bool](Get-Command $Cmd -ErrorAction SilentlyContinue)
}

$python = $null
foreach ($c in @('py', 'python3', 'python')) {
    if (Have-Cmd $c) {
        $python = $c
        break
    }
}

function Invoke-Pipx {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)
    if (Have-Cmd 'pipx') {
        & pipx @Args
    } elseif ($python) {
        & $python -m pipx @Args
    } else {
        return $false
    }
    return $?
}

Write-Info "Checking pipx..."
if (-not (Have-Cmd 'pipx') -and -not $python) {
    Write-Host "pipx is not installed; nothing to uninstall via pipx."
    exit 0
}

Invoke-Pipx uninstall embed-log 2>&1 | Out-Null
if ($?) {
    Write-OK "embed-log uninstalled."
    exit 0
}

Write-Host "embed-log is not installed via pipx."
