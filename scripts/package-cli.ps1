param(
    [Parameter(Mandatory = $true)]
    [string]$Target,

    [string]$BinPath = "target\release\embed-log.exe",
    [string]$DistDir = "dist"
)

$ErrorActionPreference = "Stop"
$Bin = "embed-log.exe"

if (!(Test-Path $BinPath)) {
    throw "Binary not found: $BinPath. Build it first, e.g. cargo build --locked --release --package embed-log-cli --bin embed-log"
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

try {
    Copy-Item $BinPath (Join-Path $TempDir $Bin)
    $Archive = Join-Path $DistDir "embed-log-$Target.zip"
    if (Test-Path $Archive) {
        Remove-Item $Archive -Force
    }
    Compress-Archive -Path (Join-Path $TempDir $Bin) -DestinationPath $Archive
    Write-Output $Archive
} finally {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
