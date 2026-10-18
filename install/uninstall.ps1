<#
.SYNOPSIS
  Uninstall OpenBark for the current Windows user.

.DESCRIPTION
  Removes the program, its shortcuts and its PATH entry. Your encounters,
  models and settings in %LOCALAPPDATA%\OpenBark are kept unless you pass
  -RemoveData (you're asked to confirm, unless you also pass -Yes).

  Removing data also deletes this computer's encryption key. Encounters you
  copied elsewhere can then only be opened with your recovery key
  (Settings > Encryption > Show recovery key), so save it first.
#>
[CmdletBinding()]
param(
  # Also delete encounters, models, templates, team members and the encryption key.
  [switch]$RemoveData,
  # Don't ask for confirmation.
  [switch]$Yes
)

$ErrorActionPreference = "Stop"
$AppDir = Join-Path $env:LOCALAPPDATA "Programs\OpenBark"
$DataDir = Join-Path $env:LOCALAPPDATA "OpenBark"
$Exe = Join-Path $AppDir "openbark.exe"

function Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }

# Double-clicked (no options): offer to delete data too; keeping it is the default.
$ask = $RemoveData -or -not ($PSBoundParameters.ContainsKey("RemoveData") -or $Yes)
if ($ask -and -not $Yes) {
  Write-Host "Do you also want to delete everything OpenBark stored on this computer?" -ForegroundColor Yellow
  Write-Host "  $DataDir (encounters, recordings, notes, models, templates, team members)"
  Write-Host "  and the encryption key. Without your recovery key, copies of encounters can't be opened."
  $answer = Read-Host "Type DELETE to remove your data, or press Enter to keep it"
  $RemoveData = $answer -ceq "DELETE"
  if (-not $RemoveData) { Write-Host "Keeping your data." }
}

Step "Stopping OpenBark"
Get-Process openbark -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.Path.StartsWith($AppDir, [StringComparison]::OrdinalIgnoreCase) } |
  Stop-Process -Force
Start-Sleep -Milliseconds 500

if ($RemoveData -and (Test-Path $Exe)) {
  Step "Deleting the encryption key"
  & $Exe key forget --yes
}

Step "Removing shortcuts"
Remove-Item -Recurse -Force (Join-Path ([Environment]::GetFolderPath("Programs")) "OpenBark") -ErrorAction SilentlyContinue
Remove-Item -Force (Join-Path ([Environment]::GetFolderPath("Desktop")) "OpenBark.lnk") -ErrorAction SilentlyContinue

Step "Removing OpenBark from PATH"
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath) {
  $kept = ($userPath -split ";" | Where-Object { $_ -and $_ -ne $AppDir }) -join ";"
  [Environment]::SetEnvironmentVariable("Path", $kept, "User")
}

Step "Removing the program"
# This script may be running from the program folder: copy out and finish there if so.
if ($PSScriptRoot -and $PSScriptRoot.StartsWith($AppDir, [StringComparison]::OrdinalIgnoreCase)) {
  Get-ChildItem $AppDir -Exclude "uninstall.ps1" | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
  Start-Process powershell.exe -WindowStyle Hidden -ArgumentList "-NoProfile -Command `"Start-Sleep 2; Remove-Item -Recurse -Force '$AppDir'`""
} else {
  Remove-Item -Recurse -Force $AppDir -ErrorAction SilentlyContinue
}

if ($RemoveData) {
  Step "Deleting your data"
  Remove-Item -Recurse -Force $DataDir -ErrorAction SilentlyContinue
  if ($env:OPENBARK_SESSIONS_DIR -or $env:OPENBARK_MODELS_DIR) {
    Write-Host "  OPENBARK_SESSIONS_DIR or OPENBARK_MODELS_DIR points elsewhere; those folders were left alone." -ForegroundColor Yellow
  }
}

Write-Host ""
Write-Host "OpenBark is uninstalled." -ForegroundColor Green
if (-not $RemoveData -and (Test-Path $DataDir)) {
  Write-Host "  Your data is still in $DataDir. Reinstall to use it again, or run with -RemoveData to delete it."
}
