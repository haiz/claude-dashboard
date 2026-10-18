param(
    [Parameter(Mandatory)] [string] $Out,
    # Store upload: drop the manifest "key" (the store assigns the item's own id).
    [switch] $Store
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

# Allowlist only. Never glob the extension dir: it can hold a local extension-private.pem.
# app/src/ext_install.rs embeds the same list (a test checks).
$allow = @(
    'manifest.json', 'background.js', 'popup.html', 'popup.js',
    'lib/extension-id.js', 'lib/status.js', 'lib/sync.js',
    'icons/icon-16.png', 'icons/icon-32.png', 'icons/icon-48.png', 'icons/icon-128.png'
)
$src = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\extension'))
foreach ($f in $allow) {
    if (-not (Test-Path -LiteralPath (Join-Path $src $f) -PathType Leaf)) { throw "missing extension file: $f" }
}

$Out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)
$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
if (Test-Path -LiteralPath $Out) { Remove-Item -LiteralPath $Out -Force }

# Write entries directly so names always use '/' (Compress-Archive in 5.1 can emit '\').
$zip = [System.IO.Compression.ZipFile]::Open($Out, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($f in $allow) {
        if ($Store -and $f -eq 'manifest.json') {
            # Remove the "key" line as text, so the rest of the manifest stays byte-for-byte.
            $text = (Get-Content -LiteralPath (Join-Path $src $f) -Raw) -replace '(?m)^\s*"key":\s*"[^"]*",\r?\n', ''
            if ($text -match '"key"') { throw 'manifest "key" not removed' }
            $entry = $zip.CreateEntry($f, [System.IO.Compression.CompressionLevel]::Optimal)
            $w = New-Object System.IO.StreamWriter($entry.Open(), (New-Object System.Text.UTF8Encoding($false)))
            try { $w.Write($text) } finally { $w.Dispose() }
            continue
        }
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $zip, (Join-Path $src $f), $f, [System.IO.Compression.CompressionLevel]::Optimal)
    }
} finally { $zip.Dispose() }
Write-Host "wrote $Out"
