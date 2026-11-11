# Rustman installer for Windows (PowerShell 5.1 or later).
#
#   irm https://github.com/frammawiliansyah/rustman/releases/latest/download/install.ps1 | iex
#
# Installs the CLI and the desktop app under %LOCALAPPDATA%\Rustman, adds them to
# your user PATH and creates a Start Menu shortcut. No administrator rights needed.
#
# Environment variables:
#   $env:RUSTMAN_VERSION = "vX.Y.Z"   install a specific version (default: latest)
#   $env:RUSTMAN_NO_DESKTOP = "1"     install the CLI only
#   $env:RUSTMAN_UNINSTALL = "1"      remove everything this script installed
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo = "frammawiliansyah/rustman"
$target = "x86_64-pc-windows-msvc"
$dir = Join-Path $env:LOCALAPPDATA "Rustman"
$binDir = Join-Path $dir "bin"
$appDir = Join-Path $dir "desktop"
$shortcut = Join-Path ([Environment]::GetFolderPath("Programs")) "Rustman.lnk"

function Update-UserPath($add, $remove) {
    $parts = @(([Environment]::GetEnvironmentVariable("Path", "User") -split ";") | Where-Object { $_ -and ($_ -ne $remove) })
    if ($add -and ($parts -notcontains $add)) { $parts += $add }
    [Environment]::SetEnvironmentVariable("Path", ($parts -join ";"), "User")
}

if ($env:RUSTMAN_UNINSTALL -eq "1") {
    Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
    Remove-Item -Force $shortcut -ErrorAction SilentlyContinue
    Update-UserPath $null $binDir
    Update-UserPath $null $appDir
    Write-Host "Rustman removed. Your data in ~\.rustman was kept."
    return
}

if (-not [Environment]::Is64BitOperatingSystem) { throw "Rustman requires 64-bit Windows." }

$version = if ($env:RUSTMAN_VERSION) { $env:RUSTMAN_VERSION } else { "latest" }
$base = if ($version -eq "latest") { "https://github.com/$repo/releases/latest/download" } else { "https://github.com/$repo/releases/download/$version" }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("rustman-" + [Guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
try {
    Invoke-WebRequest "$base/SHA256SUMS.txt" -OutFile (Join-Path $tmp "SHA256SUMS.txt")
    $sums = Get-Content (Join-Path $tmp "SHA256SUMS.txt")

    function Get-Verified($name) {
        Write-Host "Downloading $name..."
        $path = Join-Path $tmp $name
        Invoke-WebRequest "$base/$name" -OutFile $path
        $line = $sums | Where-Object { $_ -match " \*?$([regex]::Escape($name))$" } | Select-Object -First 1
        if (-not $line) { throw "No checksum listed for $name" }
        $want = ($line -split "\s+")[0]
        if ((Get-FileHash $path -Algorithm SHA256).Hash -ne $want.ToUpper()) { throw "Checksum mismatch for $name" }
        return $path
    }

    $cli = Get-Verified "rustman-$target.zip"
    Expand-Archive $cli -DestinationPath $tmp -Force
    New-Item -ItemType Directory $binDir -Force | Out-Null
    # `rustman update` runs this script while rustman.exe is still running. Windows cannot
    # overwrite a running program but allows renaming it, so move the old one aside.
    Get-ChildItem $binDir -Filter "*.old" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    $current = Join-Path $binDir "rustman.exe"
    if (Test-Path $current) { Move-Item $current "$current.old" -Force }
    Copy-Item (Join-Path $tmp "rustman-$target\rustman.exe") $binDir -Force
    Update-UserPath $binDir $null

    if ($env:RUSTMAN_NO_DESKTOP -ne "1") {
        $app = Get-Verified "rustman-desktop-$target.zip"
        Remove-Item -Recurse -Force $appDir -ErrorAction SilentlyContinue
        Expand-Archive $app -DestinationPath $tmp -Force
        New-Item -ItemType Directory $appDir -Force | Out-Null
        Copy-Item (Join-Path $tmp "rustman-desktop-$target\*") $appDir -Recurse -Force
        Update-UserPath $appDir $null

        $ws = New-Object -ComObject WScript.Shell
        $lnk = $ws.CreateShortcut($shortcut)
        $lnk.TargetPath = Join-Path $appDir "rustman-desktop.exe"
        $lnk.WorkingDirectory = $appDir
        $ico = Join-Path $appDir "rustman.ico"
        if (Test-Path $ico) { $lnk.IconLocation = $ico }
        $lnk.Save()
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "Installed Rustman ($version) in $dir"
Write-Host "Open a new terminal and try: rustman --version"
if ($env:RUSTMAN_NO_DESKTOP -ne "1") { Write-Host "Desktop app: search for Rustman in the Start Menu, or run rustman-desktop" }
