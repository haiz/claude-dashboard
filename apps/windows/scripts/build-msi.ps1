<#
.SYNOPSIS
  Builds apps/windows/target/wix/ClaudeDashboard-x64.msi (release binaries + cargo-wix).
.PARAMETER WixDir
  WiX 3.14 root (contains bin\candle.exe). Defaults to $env:WIX, then
  $env:LOCALAPPDATA\Programs\wix314.
.PARAMETER InstallVersion
  Override the MSI version (upgrade testing only); defaults to the Cargo version.
#>
param(
    [string] $WixDir = $(if ($env:WIX) { $env:WIX } else { Join-Path $env:LOCALAPPDATA 'Programs\wix314' }),
    [string] $InstallVersion = ''
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
if (-not (Test-Path (Join-Path $WixDir 'bin\candle.exe'))) { throw "WiX not found: $WixDir\bin\candle.exe" }
$env:WIX = (Resolve-Path $WixDir).Path
Push-Location $root
try {
    cargo build --release --locked --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $out = Join-Path $root 'target\wix\ClaudeDashboard-x64.msi'
    New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
    $wixArgs = @('wix', '--no-build', '--nocapture', '--package', 'claude-dashboard', '--output', $out)
    if ($InstallVersion) { $wixArgs += @('--install-version', $InstallVersion) }
    cargo @wixArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo wix failed" }
    Write-Host "Built $out"
} finally {
    Pop-Location
}
