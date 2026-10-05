param([Parameter(Mandatory)] [string] $Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

# Allowlist only. Never glob the extension dir: it can hold a local extension-private.pem.
$allow = @(
    'manifest.json', 'background.js', 'popup.html', 'popup.js',
    'lib/extension-id.js', 'lib/status.js', 'lib/sync.js'
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
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $zip, (Join-Path $src $f), $f, [System.IO.Compression.CompressionLevel]::Optimal)
    }
} finally { $zip.Dispose() }
Write-Host "wrote $Out"
