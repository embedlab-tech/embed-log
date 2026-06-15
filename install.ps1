param(
    [string]$Repo = $(if ($env:EMBED_LOG_REPO) { $env:EMBED_LOG_REPO } else { "krezolekcoder/embed-log" }),
    [string]$Version = $(if ($env:EMBED_LOG_VERSION) { $env:EMBED_LOG_VERSION } else { "latest" }),
    [string]$InstallDir = $(if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\embed-log\bin" }),
    [switch]$NoModifyPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Info($Message) {
    Write-Host $Message
}

function Fail($Message) {
    throw "error: $Message"
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

    Write-Info "Installed embed-log to $Destination"

    $NormalizedInstallDir = $InstallDir.TrimEnd("\")
    $PathEntries = @($env:Path -split ";" | Where-Object { $_ } | ForEach-Object { $_.Trim('"').TrimEnd("\") })
    $OnProcessPath = $PathEntries | Where-Object { $_ -ieq $NormalizedInstallDir } | Select-Object -First 1

    if ($NoModifyPath) {
        if (-not $OnProcessPath) {
            Write-Info ""
            Write-Info "Note: $InstallDir is not on PATH. Add it manually or run:"
            Write-Info "  `$env:Path = `"$InstallDir;`$env:Path`""
        }
    } else {
        $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if (-not $UserPath) { $UserPath = "" }
        $UserPathEntries = @($UserPath -split ";" | Where-Object { $_ } | ForEach-Object { $_.Trim('"').TrimEnd("\") })
        $OnUserPath = $UserPathEntries | Where-Object { $_ -ieq $NormalizedInstallDir } | Select-Object -First 1

        if (-not $OnUserPath) {
            $NewUserPath = if ($UserPath.Trim()) { "$UserPath;$InstallDir" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
            Write-Info "Added $InstallDir to your user PATH. Open a new terminal to use embed-log from anywhere."
        }

        if (-not $OnProcessPath) {
            $env:Path = "$InstallDir;$env:Path"
        }
    }

    Write-Info ""
    Write-Info "Try: embed-log --help"
} finally {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
