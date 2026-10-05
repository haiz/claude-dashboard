<#
.SYNOPSIS
  Registers the Claude Dashboard native-messaging host for local development.

.DESCRIPTION
  Writes a resolved host manifest (absolute exe path, the given extension id in
  allowed_origins) next to the built bridge, then points Chrome and Edge at it
  via the per-user registry key each reads for native-messaging hosts. Idempotent.

.PARAMETER ExePath
  Path to the built claude-dashboard-bridge.exe.

.PARAMETER ExtensionId
  The unpacked extension's fixed id (from scripts/gen-key.mjs).

.EXAMPLE
  .\register-dev-host.ps1 -ExePath ..\bridge\target\debug\claude-dashboard-bridge.exe -ExtensionId abcdef...
#>
param(
    [Parameter(Mandatory = $true)] [string] $ExePath,
    [Parameter(Mandatory = $true)] [string] $ExtensionId
)

$ErrorActionPreference = 'Stop'
$HostName = 'com.claude_dashboard.bridge'

$exe = (Resolve-Path $ExePath).Path
$exeDir = Split-Path $exe -Parent
$manifestPath = Join-Path $exeDir "$HostName.json"

$manifest = [ordered]@{
    name           = $HostName
    description    = 'Claude Dashboard native messaging host'
    path           = $exe
    type           = 'stdio'
    allowed_origins = @("chrome-extension://$ExtensionId/")
}
$manifest | ConvertTo-Json -Depth 5 | Out-File -FilePath $manifestPath -Encoding utf8
Write-Host "Wrote manifest: $manifestPath"

foreach ($root in @(
        "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName",
        "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$HostName"
    )) {
    New-Item -Path $root -Force | Out-Null
    Set-ItemProperty -Path $root -Name '(default)' -Value $manifestPath
    Write-Host "Registered: $root"
}

Write-Host "Done. Reload the extension if it was already running."
