param(
    [string]$Repo = $(if ($env:EMBED_LOG_REPO) { $env:EMBED_LOG_REPO } else { "krezolekcoder/embed-log" }),
    [string]$Version = $(if ($env:EMBED_LOG_VERSION) { $env:EMBED_LOG_VERSION } else { "latest" }),
    [string]$InstallDir = $(if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\embed-log\bin" }),
    [switch]$NoModifyPath,
    [switch]$NoPrompt
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Info($Message) {
    Write-Host $Message
}

function Fail($Message) {
    throw "error: $Message"
}

function Test-InstallerInteractive {
    if ($NoPrompt -or $env:EMBED_LOG_NO_PROMPT -eq "1" -or $env:CI -eq "true") {
        return $false
    }
    return [Environment]::UserInteractive
}

function Confirm-YesNo($Message, [bool]$DefaultYes = $true) {
    if (-not (Test-InstallerInteractive)) {
        return $DefaultYes
    }

    $Suffix = if ($DefaultYes) { "[Y/n]" } else { "[y/N]" }
    while ($true) {
        $Answer = Read-Host "$Message $Suffix"
        if ([string]::IsNullOrWhiteSpace($Answer)) {
            return $DefaultYes
        }
        switch -Regex ($Answer.Trim()) {
            "^(y|yes)$" { return $true }
            "^(n|no)$" { return $false }
            default { Write-Info "Please answer y or n." }
        }
    }
}

function ConvertTo-OptionalYesNo($Value) {
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $null
    }
    switch -Regex ($Value.Trim()) {
        "^(1|true|yes|y)$" { return $true }
        "^(0|false|no|n)$" { return $false }
        default { return $null }
    }
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Fail "this installer supports Windows only"
}

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    # Not needed on modern PowerShell, keep compatibility best-effort.
}

$Bin = "embed-log.exe"
$RuntimeArch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
switch -Regex ($RuntimeArch) {
    "^(AMD64|x86_64)$" { $Target = "x86_64-pc-windows-msvc"; break }
    default { Fail "no prebuilt embed-log CLI release is currently published for Windows $RuntimeArch" }
}

$ArchiveName = "embed-log-$Target.zip"
if ($Version -eq "latest") {
    $BaseUrl = "https://github.com/$Repo/releases/latest/download"
} else {
    $BaseUrl = "https://github.com/$Repo/releases/download/$Version"
}

if ($env:EMBED_LOG_BASE_URL) {
    $BaseUrl = $env:EMBED_LOG_BASE_URL
}

Write-Info "Embed-log Installer"
Write-Info "  Embedded log viewer and collection CLI."
Write-Info ""
Write-Info "Target: $Target"
Write-Info "Install directory: $InstallDir"
Write-Info ""

if (Test-InstallerInteractive) {
    Write-Info "Choose an action:"
    Write-Info ""
    Write-Info "  y    Install embed-log (default)"
    Write-Info "  n    Do nothing"
    Write-Info ""
    if (Confirm-YesNo "Install embed-log now?" $true) {
        Write-Info "Will install embed-log."
        Write-Info ""
    } else {
        Write-Info "Nothing changed."
        exit 0
    }
}

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("embed-log-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

try {
    $ArchivePath = Join-Path $TempDir $ArchiveName
    $ChecksumsPath = Join-Path $TempDir "SHA256SUMS"

    Write-Info "Installing embed-log for $Target"
    Write-Info "Downloading $ArchiveName"
    Invoke-WebRequest -Uri "$BaseUrl/$ArchiveName" -OutFile $ArchivePath -UseBasicParsing
    Invoke-WebRequest -Uri "$BaseUrl/SHA256SUMS" -OutFile $ChecksumsPath -UseBasicParsing

    Write-Info "Verifying checksum"
    $ChecksumLine = Get-Content $ChecksumsPath | Where-Object { $_ -match "\s+$([regex]::Escape($ArchiveName))$" } | Select-Object -First 1
    if (-not $ChecksumLine) {
        Fail "checksum entry for $ArchiveName was not found in SHA256SUMS"
    }

    $ExpectedHash = (($ChecksumLine -split "\s+") | Where-Object { $_ })[0].ToUpperInvariant()
    $ActualHash = (Get-FileHash -Algorithm SHA256 -Path $ArchivePath).Hash.ToUpperInvariant()
    if ($ActualHash -ne $ExpectedHash) {
        Fail "checksum mismatch for $ArchiveName`nexpected: $ExpectedHash`nactual:   $ActualHash"
    }

    Expand-Archive -Path $ArchivePath -DestinationPath $TempDir -Force
    $ExtractedBin = Join-Path $TempDir $Bin
    if (-not (Test-Path $ExtractedBin)) {
        Fail "archive did not contain $Bin"
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $Destination = Join-Path $InstallDir $Bin
    Copy-Item $ExtractedBin $Destination -Force

    Write-Info ""
    Write-Info "embed-log was installed successfully."
    Write-Info "Installed embed-log to $Destination"

    $ResolvedCommand = Get-Command "embed-log" -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $ResolvedPath = if ($ResolvedCommand) { $ResolvedCommand.Definition } else { $null }

    $NormalizedInstallDir = $InstallDir.TrimEnd("\")
    $PathEntries = @($env:Path -split ";" | Where-Object { $_ } | ForEach-Object { $_.Trim('"').TrimEnd("\") })
    $OnProcessPath = $PathEntries | Where-Object { $_ -ieq $NormalizedInstallDir } | Select-Object -First 1

    if ($NoModifyPath) {
        if (-not $OnProcessPath) {
            Write-Info ""
            Write-Info "embed-log was installed, but $InstallDir is not on PATH yet."
            Write-Info "Add it manually or run:"
            Write-Info "  `$env:Path = `"$InstallDir;`$env:Path`""
        }
    } else {
        $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if (-not $UserPath) { $UserPath = "" }
        $UserPathEntries = @($UserPath -split ";" | Where-Object { $_ } | ForEach-Object { $_.Trim('"').TrimEnd("\") })
        $OnUserPath = $UserPathEntries | Where-Object { $_ -ieq $NormalizedInstallDir } | Select-Object -First 1

        $PathNeedsUpdate = (-not $OnUserPath) -or ($ResolvedPath -and ($ResolvedPath -ine $Destination))
        $ShouldModifyPath = $false
        if ($PathNeedsUpdate) {
            Write-Info ""
            if ($ResolvedPath -and ($ResolvedPath -ine $Destination)) {
                Write-Info "embed-log was installed, but your shell is not using that install yet."
                Write-Info "Your shell currently resolves embed-log to: $ResolvedPath"
            } elseif (-not $OnProcessPath) {
                Write-Info "embed-log was installed, but $InstallDir is not on PATH yet."
            }

            $PathChoice = ConvertTo-OptionalYesNo $env:EMBED_LOG_UPDATE_PATH
            if ($null -ne $PathChoice) {
                $ShouldModifyPath = $PathChoice
            } elseif (Test-InstallerInteractive) {
                $ShouldModifyPath = Confirm-YesNo "Add $InstallDir to your user PATH now?" $true
            } else {
                # Preserve the old Windows behavior for non-interactive installs.
                $ShouldModifyPath = $true
            }
        }

        if ($ShouldModifyPath) {
            $FilteredUserPathEntries = @($UserPath -split ";" | Where-Object { $_ } | Where-Object { $_.Trim('"').TrimEnd("\") -ine $NormalizedInstallDir })
            $NewUserPath = if ($FilteredUserPathEntries.Count -gt 0) { "$InstallDir;$($FilteredUserPathEntries -join ';')" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
            Write-Info "Added $InstallDir to your user PATH. Open a new terminal to use embed-log from anywhere."
        }

        if (-not $OnProcessPath) {
            $env:Path = "$InstallDir;$env:Path"
        }
    }

    Write-Info ""
    Write-Info "Then run: embed-log"
} finally {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
