[CmdletBinding()]
param([string]$FFmpegDirectory, [switch]$SkipShell)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cargoExe = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargoExe)) { $cargoExe = (Get-Command cargo.exe -ErrorAction Stop).Source }
if (-not $FFmpegDirectory) { $FFmpegDirectory = Split-Path -Parent (Get-Command ffmpeg.exe -ErrorAction Stop).Source }
foreach ($name in @('ffmpeg.exe','ffprobe.exe')) {
    if (-not (Test-Path -LiteralPath (Join-Path $FFmpegDirectory $name))) { throw "Missing $name in $FFmpegDirectory" }
}
Push-Location $projectRoot
try {
    & $cargoExe build --release --locked --manifest-path cli/Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw 'CLI build failed.' }
    if (-not (Test-Path -LiteralPath 'node_modules')) {
        & npm.cmd ci
        if ($LASTEXITCODE -ne 0) { throw 'npm install failed.' }
    }
    $env:PATH = "$(Split-Path -Parent $cargoExe);$env:PATH"
    & npm.cmd run tauri -- build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw 'GUI build failed.' }
    $releaseRoot = Join-Path $projectRoot 'release'
    New-Item -ItemType Directory -Path $releaseRoot -Force | Out-Null
    Copy-Item -LiteralPath 'cli\target\release\UtilitiesCli.exe' -Destination $releaseRoot -Force
    Copy-Item -LiteralPath 'src-tauri\target\release\Utilities.exe' -Destination $releaseRoot -Force
    foreach ($name in @('ffmpeg.exe','ffprobe.exe')) { Copy-Item -LiteralPath (Join-Path $FFmpegDirectory $name) -Destination $releaseRoot -Force }
    $notice = @'
This local release includes FFmpeg and ffprobe from the FFmpeg directory supplied to build.ps1.
FFmpeg project: https://ffmpeg.org/
License information: run ffmpeg.exe -L
Source: https://ffmpeg.org/download.html and the provider of the supplied build.
The supplied GPL build's licensing applies to FFmpeg and ffprobe; no project license has been chosen yet.
'@
    Set-Content -LiteralPath (Join-Path $releaseRoot 'THIRD-PARTY.txt') -Value $notice -Encoding UTF8
    if (-not $SkipShell) { & (Join-Path $PSScriptRoot 'build-shell.ps1') }
    Write-Output "Built $releaseRoot\Utilities.exe and UtilitiesCli.exe"
}
finally { Pop-Location }
