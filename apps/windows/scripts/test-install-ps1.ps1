# Unit test for install.ps1's helpers: architecture check and asset-URL
# resolution. No network, nothing installed: the script is dot-sourced with
# its test guard set, so only the definitions load.
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot '..\..\..\install.ps1'

function Fail([string]$Message) { Write-Error "FAIL: $Message"; exit 1 }

$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $script), [ref]$tokens, [ref]$errors)
if ($errors) { Fail "install.ps1 does not parse: $($errors[0].Message)" }

$text = Get-Content $script -Raw
if ($text -notmatch 'CLAUDE_DASHBOARD_INSTALL_TEST') { Fail 'install.ps1 has no test guard' }
# Windows PowerShell 5.1 reads a BOM-less file as ANSI, and `irm | iex` users hit 5.1.
if ($text -match '[^\x00-\x7F]') { Fail 'install.ps1 must be ASCII-only' }
# `exit` under `irm | iex` would close the user's own shell.
$exits = $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.ExitStatementAst] }, $true)
if ($exits) { Fail 'install.ps1 must not call exit' }

$env:CLAUDE_DASHBOARD_INSTALL_TEST = '1'
try { . $script } finally { Remove-Item Env:CLAUDE_DASHBOARD_INSTALL_TEST }

if ((Test-CdArchitecture 'AMD64') -ne 'x64') { Fail 'AMD64 should map to x64' }
if ((Test-CdArchitecture 'ARM64') -ne 'arm64') { Fail 'ARM64 should map to arm64' }
$threw = $false
try { Test-CdArchitecture 'x86' | Out-Null } catch { $threw = $true }
if (-not $threw) { Fail 'x86 was accepted' }

# The collision case: the macOS app zip is uploaded first and is also a .zip.
$release = '{"tag_name":"v1.19.0","assets":[
 {"name":"ClaudeDashboard.app.zip","browser_download_url":"https://example.test/v1.19.0/ClaudeDashboard.app.zip"},
 {"name":"claude-dashboard-cli.tar.gz","browser_download_url":"https://example.test/v1.19.0/claude-dashboard-cli.tar.gz"},
 {"name":"ClaudeDashboard-x64.msi","browser_download_url":"https://example.test/v1.19.0/ClaudeDashboard-x64.msi"},
 {"name":"claude-dashboard-extension.zip","browser_download_url":"https://example.test/v1.19.0/claude-dashboard-extension.zip"}]}' | ConvertFrom-Json

$got = Get-CdAssetUrl $release $ClaudeDashboardMsiAsset
if ($got -ne 'https://example.test/v1.19.0/ClaudeDashboard-x64.msi') { Fail "msi resolved to '$got'" }
$got = Get-CdAssetUrl $release $ClaudeDashboardExtensionAsset
if ($got -ne 'https://example.test/v1.19.0/claude-dashboard-extension.zip') { Fail "extension resolved to '$got'" }

# The release is published before the Windows workflow uploads anything.
$early = '{"tag_name":"v1.19.0","assets":[
 {"name":"ClaudeDashboard.app.zip","browser_download_url":"https://example.test/v1.19.0/ClaudeDashboard.app.zip"}]}' | ConvertFrom-Json
if ($null -ne (Get-CdAssetUrl $early $ClaudeDashboardMsiAsset)) { Fail 'missing msi should resolve to $null' }
$empty = '{"tag_name":"v1.19.0","assets":[]}' | ConvertFrom-Json
if ($null -ne (Get-CdAssetUrl $empty $ClaudeDashboardMsiAsset)) { Fail 'empty assets should resolve to $null' }

# The names must match what release-windows.yml uploads.
$workflow = Get-Content (Join-Path $PSScriptRoot '..\..\..\.github\workflows\release-windows.yml') -Raw
foreach ($name in @($ClaudeDashboardMsiAsset, $ClaudeDashboardExtensionAsset)) {
    if (-not $workflow.Contains($name)) { Fail "release-windows.yml does not upload $name" }
}

Write-Host 'PASS'
