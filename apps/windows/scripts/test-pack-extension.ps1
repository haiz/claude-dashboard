$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$expected = @('background.js','lib/extension-id.js','lib/status.js','lib/sync.js','manifest.json','popup.html','popup.js') | Sort-Object
$out = Join-Path ([System.IO.Path]::GetTempPath()) ("cd-ext-test-" + [guid]::NewGuid() + ".zip")
& (Join-Path $PSScriptRoot 'pack-extension.ps1') -Out $out
$zip = [System.IO.Compression.ZipFile]::OpenRead($out)
try {
    $names = $zip.Entries | ForEach-Object { $_.FullName -replace '\\','/' } | Where-Object { -not $_.EndsWith('/') } | Sort-Object
    if (($names -join ',') -ne ($expected -join ',')) { Write-Error "zip entries: $($names -join ', ')"; exit 1 }
    # Chrome needs forward-slash entry names as stored, not just after normalising.
    $raw = $zip.Entries | Where-Object { $_.FullName.Contains('\') }
    if ($raw) { Write-Error "backslash entry names: $($raw.FullName -join ', ')"; exit 1 }
    $reader = New-Object System.IO.StreamReader(($zip.GetEntry('manifest.json')).Open())
    $inner = $reader.ReadToEnd() | ConvertFrom-Json; $reader.Close()
    $repo = Get-Content (Join-Path $PSScriptRoot '..\extension\manifest.json') -Raw | ConvertFrom-Json
    if ($inner.version -ne $repo.version) { Write-Error "version $($inner.version) != $($repo.version)"; exit 1 }
} finally { $zip.Dispose(); Remove-Item $out -ErrorAction SilentlyContinue }
Write-Host 'PASS'
