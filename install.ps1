# Claude Dashboard installer for Windows
# Usage: irm https://raw.githubusercontent.com/haiz/claude-dashboard/main/install.ps1 | iex
#
# Installs the latest per-user MSI (no admin prompt), unpacks the browser
# extension next to the user data, and starts the app. Runs under Windows
# PowerShell 5.1 and PowerShell 7. Never calls `exit`: under `irm | iex` that
# would close the user's own shell, so failures `throw` instead.

$ClaudeDashboardRepo = 'haiz/claude-dashboard'
$ClaudeDashboardMsiAsset = 'ClaudeDashboard-x64.msi'
$ClaudeDashboardExtensionAsset = 'claude-dashboard-extension.zip'

function Write-CdStep([string]$Message) { Write-Host '==> ' -ForegroundColor Blue -NoNewline; Write-Host $Message }
function Write-CdOk([string]$Message)   { Write-Host '==> ' -ForegroundColor Green -NoNewline; Write-Host $Message }
function Write-CdWarn([string]$Message) { Write-Host '==> ' -ForegroundColor Yellow -NoNewline; Write-Host $Message }

# The MSI is x64. ARM64 Windows 11 runs it under emulation; 32-bit Windows cannot.
function Test-CdArchitecture([string]$Arch) {
    switch ($Arch) {
        'AMD64' { return 'x64' }
        'ARM64' { return 'arm64' }
        default { throw "Unsupported architecture: $Arch. Claude Dashboard ships a 64-bit (x64) installer only." }
    }
}

# Resolve one exact asset name out of a parsed releases API payload. Matching the
# full name matters: the macOS release also carries a .zip
# (ClaudeDashboard.app.zip), so a suffix match could pick it. $null if absent.
function Get-CdAssetUrl($Release, [string]$Name) {
    foreach ($asset in @($Release.assets)) {
        if ($asset.name -eq $Name) { return $asset.browser_download_url }
    }
    return $null
}

function Install-ClaudeDashboard {
    $ErrorActionPreference = 'Stop'
    # Invoke-WebRequest's progress bar makes 5.1 downloads crawl.
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 may default to TLS 1.0, which GitHub refuses.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    if ($env:OS -ne 'Windows_NT') {
        throw 'This installer is for Windows. On macOS or Linux run: curl -fsSL https://raw.githubusercontent.com/haiz/claude-dashboard/main/install.sh | bash'
    }
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    if ((Test-CdArchitecture $arch) -eq 'arm64') {
        Write-CdWarn 'ARM64 Windows detected: the x64 build runs under emulation.'
    }

    Write-CdStep 'Fetching latest release...'
    $headers = @{ 'User-Agent' = 'claude-dashboard-installer'; 'Accept' = 'application/vnd.github+json' }
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$ClaudeDashboardRepo/releases/latest" -Headers $headers -UseBasicParsing
    } catch {
        throw "Failed to fetch release info. Check your internet connection. ($($_.Exception.Message))"
    }
    $version = $release.tag_name
    $msiUrl = Get-CdAssetUrl $release $ClaudeDashboardMsiAsset
    if (-not $msiUrl) {
        throw "Release $version has no $ClaudeDashboardMsiAsset yet. The Windows build may still be running - try again in a few minutes, or check https://github.com/$ClaudeDashboardRepo/releases"
    }
    Write-CdStep "Found Claude Dashboard $version"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('claude-dashboard-install-' + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $msi = Join-Path $tmp $ClaudeDashboardMsiAsset
        Write-CdStep 'Downloading installer...'
        Invoke-WebRequest -Uri $msiUrl -OutFile $msi -Headers $headers -UseBasicParsing

        # A running copy would hold its exe open and turn the upgrade into a
        # reboot-pending one. It lives in the tray and keeps no unsaved state.
        $running = Get-Process -Name 'claude-dashboard' -ErrorAction SilentlyContinue
        if ($running) {
            Write-CdStep 'Closing the running Claude Dashboard...'
            $running | Stop-Process -Force
            $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
        }

        Write-CdStep 'Installing (per user, no admin needed)...'
        $proc = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', "`"$msi`"", '/passive', '/norestart') -Wait -PassThru
        # 3010: success, restart required.
        if ($proc.ExitCode -ne 0 -and $proc.ExitCode -ne 3010) {
            throw "msiexec failed with exit code $($proc.ExitCode)."
        }

        $extDir = Join-Path $env:LOCALAPPDATA 'claude-dashboard\extension'
        $extUrl = Get-CdAssetUrl $release $ClaudeDashboardExtensionAsset
        if ($extUrl) {
            Write-CdStep 'Unpacking the browser extension...'
            $zip = Join-Path $tmp $ClaudeDashboardExtensionAsset
            Invoke-WebRequest -Uri $extUrl -OutFile $zip -Headers $headers -UseBasicParsing
            if (Test-Path $extDir) { Remove-Item -Recurse -Force $extDir }
            New-Item -ItemType Directory -Path $extDir -Force | Out-Null
            Expand-Archive -Path $zip -DestinationPath $extDir -Force
        } else {
            $extDir = $null
            Write-CdWarn "Release $version has no $ClaudeDashboardExtensionAsset; skipping the browser extension."
        }
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }

    $exe = Join-Path $env:LOCALAPPDATA 'Programs\ClaudeDashboard\claude-dashboard.exe'
    Write-Host ''
    Write-CdOk "Claude Dashboard $version installed."
    if (Test-Path $exe) {
        Start-Process -FilePath $exe
        Write-Host '  It is running in the system tray. Start menu: Claude Dashboard'
    } else {
        Write-CdWarn "Expected $exe but it is not there; start Claude Dashboard from the Start menu."
    }
    if ($extDir) {
        Write-Host ''
        Write-Host '  Browser extension (recommended, keeps session keys in sync):'
        Write-Host '    1. Open chrome://extensions (or edge://extensions) and turn on Developer mode.'
        Write-Host '    2. Click "Load unpacked" and select:'
        Write-Host "         $extDir"
        Write-Host '    3. Open claude.ai while signed in; the account appears in the dashboard.'
    }
    Write-Host ''
}

# When dot-sourced by a test, stop here: definitions only, nothing installed.
if ($env:CLAUDE_DASHBOARD_INSTALL_TEST) { return }

Install-ClaudeDashboard
