$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$expected = @(
    'background.js','lib/extension-id.js','lib/status.js','lib/sync.js','manifest.json','popup.html','popup.js',
    'icons/icon-16.png','icons/icon-32.png','icons/icon-48.png','icons/icon-128.png'
) | Sort-Object
$repo = Get-Content (Join-Path $PSScriptRoot '..\extension\manifest.json') -Raw | ConvertFrom-Json

function Read-Manifest($zip) {
    $reader = New-Object System.IO.StreamReader(($zip.GetEntry('manifest.json')).Open())
    try { return $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Close() }
}

foreach ($store in @($false, $true)) {
    $out = Join-Path ([System.IO.Path]::GetTempPath()) ("cd-ext-test-" + [guid]::NewGuid() + ".zip")
    & (Join-Path $PSScriptRoot 'pack-extension.ps1') -Out $out -Store:$store
    $zip = [System.IO.Compression.ZipFile]::OpenRead($out)
    try {
        $names = $zip.Entries | ForEach-Object { $_.FullName -replace '\\','/' } | Where-Object { -not $_.EndsWith('/') } | Sort-Object
        if (($names -join ',') -ne ($expected -join ',')) { Write-Error "zip entries: $($names -join ', ')"; exit 1 }
        # Chrome needs forward-slash entry names as stored, not just after normalising.
        $raw = $zip.Entries | Where-Object { $_.FullName.Contains('\') }
        if ($raw) { Write-Error "backslash entry names: $($raw.FullName -join ', ')"; exit 1 }
        $inner = Read-Manifest $zip
        if ($inner.version -ne $repo.version) { Write-Error "version $($inner.version) != $($repo.version)"; exit 1 }
        # The unpacked zip keeps the fixed id's key; the store zip must not carry one.
        $hasKey = [bool]($inner.PSObject.Properties.Name -contains 'key')
        if ($hasKey -eq $store) { Write-Error "store=$store but key present=$hasKey"; exit 1 }
        if ($inner.name -ne $repo.name -or $inner.icons.'128' -ne $repo.icons.'128') { Write-Error "store=$store manifest changed beyond key"; exit 1 }
    } finally { $zip.Dispose(); Remove-Item $out -ErrorAction SilentlyContinue }
}
Write-Host 'PASS'
