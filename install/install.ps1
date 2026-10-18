<#
.SYNOPSIS
  Install OpenBark for the current Windows user (no administrator rights needed).

.DESCRIPTION
  Installs the OpenBark engine and its browser interface into
  %LOCALAPPDATA%\Programs\OpenBark, adds Start menu and desktop shortcuts, and
  optionally downloads the models it needs.

  Where the program comes from, in order:
    1. -From <folder>: a release folder holding openbark.exe and ui\
    2. the folder this script is in, if it holds openbark.exe and ui\ (an unzipped release)
    3. a build from source, when run from a checkout of the OpenBark repository

  Your encounters, models and settings live in %LOCALAPPDATA%\OpenBark and are
  never touched by installing or updating.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File install\install.ps1 -Models
#>
[CmdletBinding()]
param(
  # Folder with a prebuilt openbark.exe and ui\ (from a release zip).
  [string]$From,
  # Download the speech and note models now (about 6 GB) instead of from the app.
  [switch]$Models,
  # Don't create a desktop shortcut.
  [switch]$NoDesktopShortcut,
  # Don't add openbark to your PATH.
  [switch]$NoPath,
  # GPU support when building from source: auto, cuda, vulkan or cpu.
  [ValidateSet("auto", "cuda", "vulkan", "cpu")]
  [string]$Gpu = "auto"
)

$ErrorActionPreference = "Stop"
$AppDir = Join-Path $env:LOCALAPPDATA "Programs\OpenBark"
$DataDir = Join-Path $env:LOCALAPPDATA "OpenBark"
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = Split-Path -Parent $Here

function Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "OpenBark wasn't installed: $text" -ForegroundColor Red; exit 1 }

function Test-Release($dir) {
  return $dir -and (Test-Path (Join-Path $dir "openbark.exe")) -and (Test-Path (Join-Path $dir "ui\index.html"))
}

# 1. Find or build the program.
$Source = $null
if ($From) {
  if (-not (Test-Release $From)) { Fail "$From doesn't contain openbark.exe and ui\index.html." }
  $Source = (Resolve-Path $From).Path
} elseif (Test-Release $Here) {
  $Source = $Here
} elseif (Test-Path (Join-Path $Repo "Cargo.toml")) {
  Step "Building OpenBark from source (the first build takes 10 to 20 minutes)"
  foreach ($tool in "cargo", "node", "pnpm") {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
      Fail "$tool isn't installed. See docs/runbooks/windows.md for the one-time setup, or install from a release instead."
    }
  }
  if ($Gpu -eq "auto") {
    if (Get-Command nvcc -ErrorAction SilentlyContinue) { $Gpu = "cuda" }
    elseif ($env:VULKAN_SDK) { $Gpu = "vulkan" }
    else { $Gpu = "cpu" }
  }
  $features = @()
  if ($Gpu -ne "cpu") { $features = @("--features", $Gpu) }
  Write-Host "    graphics: $Gpu"
  Push-Location $Repo
  try {
    & cargo build --release -p os-cli @features
    if ($LASTEXITCODE -ne 0) { Fail "the engine didn't build (see the messages above)." }
    & pnpm install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { Fail "the interface's packages didn't install." }
    & pnpm --filter "@openbark/desktop" build
    if ($LASTEXITCODE -ne 0) { Fail "the interface didn't build." }
  } finally { Pop-Location }
  $Source = Join-Path $env:TEMP "openbark-install"
  if (Test-Path $Source) { Remove-Item -Recurse -Force $Source }
  New-Item -ItemType Directory -Force (Join-Path $Source "ui") | Out-Null
  Copy-Item (Join-Path $Repo "target\release\openbark.exe") $Source
  # The speech engine's libraries (sherpa-onnx, ONNX Runtime) sit beside the program.
  Copy-Item (Join-Path $Repo "target\release\*.dll") $Source
  Copy-Item -Recurse (Join-Path $Repo "apps\desktop\dist\*") (Join-Path $Source "ui")
} else {
  Fail "no OpenBark program found. Run this from an unzipped release, pass -From <folder>, or run it from the source code."
}

# 2. Stop a running copy so its files can be replaced.
Get-Process openbark -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.Path.StartsWith($AppDir, [StringComparison]::OrdinalIgnoreCase) } |
  Stop-Process -Force

# 3. Copy the program.
Step "Installing to $AppDir"
New-Item -ItemType Directory -Force $AppDir | Out-Null
if (Test-Path (Join-Path $AppDir "ui")) { Remove-Item -Recurse -Force (Join-Path $AppDir "ui") }
Copy-Item -Force (Join-Path $Source "openbark.exe") $AppDir
Copy-Item -Force (Join-Path $Source "*.dll") $AppDir -ErrorAction SilentlyContinue
if (-not (Test-Path (Join-Path $AppDir "sherpa-onnx-c-api.dll"))) {
  Fail "the speech engine's libraries (sherpa-onnx-c-api.dll and onnxruntime.dll) weren't found next to openbark.exe."
}
Copy-Item -Recurse -Force (Join-Path $Source "ui") (Join-Path $AppDir "ui")
Copy-Item -Force (Join-Path $Here "uninstall.ps1") $AppDir -ErrorAction SilentlyContinue

# 4. Launcher: starts OpenBark on this computer only (127.0.0.1) and opens the browser.
$Launcher = @'
$ErrorActionPreference = "SilentlyContinue"
$exe = Join-Path $PSScriptRoot "openbark.exe"
$url = "http://localhost:4317"
function Up { try { Invoke-WebRequest "$url/api/status" -UseBasicParsing -TimeoutSec 1 | Out-Null; $true } catch { $false } }
if (-not (Up)) {
  Start-Process -FilePath $exe -ArgumentList "serve" -WindowStyle Hidden
  for ($i = 0; $i -lt 60 -and -not (Up); $i++) { Start-Sleep -Milliseconds 500 }
}
Start-Process $url
'@
Set-Content -Path (Join-Path $AppDir "launch.ps1") -Value $Launcher -Encoding utf8
$Stopper = @'
Get-Process openbark -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$PSScriptRoot\*" } | Stop-Process -Force
'@
Set-Content -Path (Join-Path $AppDir "stop.ps1") -Value $Stopper -Encoding utf8

# 5. Shortcuts.
Step "Adding shortcuts"
$Shell = New-Object -ComObject WScript.Shell
$StartMenu = Join-Path ([Environment]::GetFolderPath("Programs")) "OpenBark"
New-Item -ItemType Directory -Force $StartMenu | Out-Null
function New-Shortcut($path, $script, $description) {
  $s = $Shell.CreateShortcut($path)
  $s.TargetPath = "powershell.exe"
  $s.Arguments = "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$(Join-Path $AppDir $script)`""
  $s.IconLocation = (Join-Path $AppDir "openbark.exe") + ",0"
  $s.WorkingDirectory = $AppDir
  $s.Description = $description
  $s.Save()
}
New-Shortcut (Join-Path $StartMenu "OpenBark.lnk") "launch.ps1" "Open OpenBark in your browser"
New-Shortcut (Join-Path $StartMenu "Stop OpenBark.lnk") "stop.ps1" "Stop OpenBark running in the background"
$u = $Shell.CreateShortcut((Join-Path $StartMenu "Uninstall OpenBark.lnk"))
$u.TargetPath = "powershell.exe"
$u.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $AppDir 'uninstall.ps1')`""
$u.Save()
if (-not $NoDesktopShortcut) {
  New-Shortcut (Join-Path ([Environment]::GetFolderPath("Desktop")) "OpenBark.lnk") "launch.ps1" "Open OpenBark in your browser"
}

# 6. PATH, so `openbark` works in a terminal.
if (-not $NoPath) {
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not ($userPath -split ";" | Where-Object { $_ -eq $AppDir })) {
    [Environment]::SetEnvironmentVariable("Path", (($userPath.TrimEnd(";") + ";" + $AppDir).TrimStart(";")), "User")
  }
}

# 7. Models (optional; the app can download them too).
if ($Models) {
  Step "Downloading models (about 6 GB; resumable if interrupted)"
  & (Join-Path $AppDir "openbark.exe") models setup
  if ($LASTEXITCODE -ne 0) { Write-Host "Some models didn't download. Open OpenBark > Models to finish." -ForegroundColor Yellow }
}

Write-Host ""
Write-Host "OpenBark is installed." -ForegroundColor Green
Write-Host "  Open it from the Start menu or desktop shortcut (it opens in your browser, on this computer only)."
Write-Host "  Your data: $DataDir"
Write-Host "  To uninstall: Start menu > OpenBark > Uninstall OpenBark"
