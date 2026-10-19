<#
Windows code signing for the release workflow (ADR-0020, decision 2).

  windows-signing.ps1 -Mode Setup -Kind Artifact
      Microsoft Artifact Signing. Downloads the pinned signing client
      (Microsoft.ArtifactSigning.Client, checked by SHA-256), finds signtool,
      writes metadata.json from AZURE_SIGNING_ENDPOINT, AZURE_SIGNING_ACCOUNT
      and AZURE_SIGNING_PROFILE, and writes a Tauri config overlay whose
      bundle.windows.signCommand runs signtool with the client's dlib.
      Sign in with azure/login before Tauri runs the command.

  windows-signing.ps1 -Mode Setup -Kind SelfSigned -Publisher "<name>"
      CI self-test only: a throwaway self-signed code-signing certificate in
      the runner's certificate store, trusted on that runner, and an overlay
      that signs with it. Same signtool, same timestamp server, same checks.

  windows-signing.ps1 -Mode Verify -Installer <setup.exe> -Publisher "<name>"
      Checks the installer, installs it silently on the runner, then checks
      every .exe and .dll installed. Each must have a valid signature from
      <name> with a timestamp. Exits non-zero otherwise.

Setup writes the overlay's path to OPENBARK_SIGN_OVERLAY in GITHUB_ENV, for
`tauri bundle --config`. The overlay never goes into tauri.conf.json, so local
builds stay unsigned. No secret is written anywhere: Artifact Signing keys stay
in Microsoft's HSMs and the sign-in comes from the Azure CLI's token.
#>
param(
  [Parameter(Mandatory = $true)][ValidateSet("Setup", "Verify")][string]$Mode,
  [ValidateSet("Artifact", "SelfSigned")][string]$Kind = "Artifact",
  [string]$Publisher = "ANI.ML Health Inc.",
  [string]$Installer = ""
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version 3

# Pins (docs/versions.md). 1.0.146 is too new for the 30-day rule.
$ClientVersion = "1.0.128"
$ClientSha256 = "74bd7d27e6ce1051409c38d9b46bc8df0400ecd643d51ffbf2ac00869061e40b"
$TimestampUrl = "http://timestamp.acs.microsoft.com"
$MinSigntool = [version]"10.0.22621.755"

$Temp = $env:RUNNER_TEMP
if (-not $Temp) { $Temp = [System.IO.Path]::GetTempPath() }

function Fail([string]$msg) {
  Write-Host "::error::$msg"
  exit 1
}

function Find-Signtool {
  $root = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
  $found = Get-ChildItem -Path $root -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Directory.Name -eq "x64" } |
    Sort-Object { [version]$_.VersionInfo.FileVersionRaw } -Descending |
    Select-Object -First 1
  if (-not $found) { Fail "signtool.exe (x64) not found under $root" }
  $v = [version]$found.VersionInfo.FileVersionRaw
  if ($v -lt $MinSigntool) { Fail "signtool $v is older than $MinSigntool, which Artifact Signing needs" }
  Write-Host "signtool $v at $($found.FullName)"
  return $found.FullName
}

function Write-Overlay([string]$signtool, [string[]]$signArgs) {
  $overlay = @{ bundle = @{ windows = @{ signCommand = @{ cmd = $signtool; args = $signArgs } } } }
  $path = Join-Path $Temp "sign.windows.json"
  # The object form of signCommand: the signtool path has spaces in it.
  $overlay | ConvertTo-Json -Depth 10 | Set-Content -Path $path -Encoding ascii
  if ($env:GITHUB_ENV) { "OPENBARK_SIGN_OVERLAY=$path" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8 }
  Write-Host "Tauri signing overlay: $path"
  Get-Content $path | Write-Host
}

function Setup-Artifact {
  foreach ($v in "AZURE_SIGNING_ENDPOINT", "AZURE_SIGNING_ACCOUNT", "AZURE_SIGNING_PROFILE") {
    if (-not [Environment]::GetEnvironmentVariable($v)) { Fail "$v is not set (a variable in the release environment)" }
  }
  # The dlib is a .NET 8 library and fails silently without the runtime.
  $runtimes = (& dotnet --list-runtimes) -join "`n"
  if ($runtimes -notmatch "Microsoft\.NETCore\.App 8\.") { Fail ".NET 8 runtime not found; the Artifact Signing dlib needs it" }

  $dir = Join-Path $Temp "artifact-signing-client"
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $pkg = Join-Path $dir "client.nupkg"
  $url = "https://api.nuget.org/v3-flatcontainer/microsoft.artifactsigning.client/$ClientVersion/microsoft.artifactsigning.client.$ClientVersion.nupkg"
  Invoke-WebRequest -Uri $url -OutFile $pkg -UseBasicParsing
  $hash = (Get-FileHash -Algorithm SHA256 -Path $pkg).Hash.ToLowerInvariant()
  if ($hash -ne $ClientSha256) { Fail "Microsoft.ArtifactSigning.Client $ClientVersion has SHA-256 $hash, expected $ClientSha256" }
  $extract = Join-Path $dir "pkg"
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
  [System.IO.Compression.ZipFile]::ExtractToDirectory($pkg, $extract)
  $dlib = Join-Path $extract "bin\x64\Azure.CodeSigning.Dlib.dll"
  if (-not (Test-Path $dlib)) { Fail "Azure.CodeSigning.Dlib.dll not found in the client package" }
  Write-Host "Artifact Signing client $ClientVersion (SHA-256 checked): $dlib"

  # Only the Azure CLI credential (from azure/login), so the dlib doesn't
  # wait on managed-identity probes on a GitHub runner.
  $metadata = [ordered]@{
    Endpoint               = $env:AZURE_SIGNING_ENDPOINT
    CodeSigningAccountName = $env:AZURE_SIGNING_ACCOUNT
    CertificateProfileName = $env:AZURE_SIGNING_PROFILE
    CorrelationId          = "$($env:GITHUB_REF_NAME)-$($env:GITHUB_RUN_ID)"
    ExcludeCredentials     = @("EnvironmentCredential", "WorkloadIdentityCredential", "ManagedIdentityCredential",
      "SharedTokenCacheCredential", "VisualStudioCredential", "VisualStudioCodeCredential",
      "AzurePowerShellCredential", "AzureDeveloperCliCredential", "InteractiveBrowserCredential")
  }
  $metaPath = Join-Path $dir "metadata.json"
  $metadata | ConvertTo-Json -Depth 5 | Set-Content -Path $metaPath -Encoding ascii

  $signtool = Find-Signtool
  Write-Overlay $signtool @("sign", "/v", "/fd", "SHA256", "/tr", $TimestampUrl, "/td", "SHA256",
    "/dlib", $dlib, "/dmdf", $metaPath, "%1")
}

function Setup-SelfSigned {
  $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=$Publisher" `
    -CertStoreLocation Cert:\CurrentUser\My -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
    -NotAfter (Get-Date).AddDays(2)
  # Trust it on this runner only, so signtool verify /pa and Windows accept it.
  $cer = Join-Path $Temp "selftest-signing.cer"
  Export-Certificate -Cert $cert -FilePath $cer | Out-Null
  Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
  Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
  Remove-Item $cer
  Write-Host "Throwaway certificate CN=$Publisher, thumbprint $($cert.Thumbprint)"
  $signtool = Find-Signtool
  Write-Overlay $signtool @("sign", "/v", "/fd", "SHA256", "/tr", $TimestampUrl, "/td", "SHA256",
    "/sha1", $cert.Thumbprint, "/s", "My", "%1")
}

function Test-Signed([string]$path, [string]$signtool) {
  & $signtool verify /pa /q $path | Out-Null
  if ($LASTEXITCODE -ne 0) {
    & $signtool verify /pa /v $path
    Fail "signtool verify /pa failed: $path"
  }
  $sig = Get-AuthenticodeSignature -FilePath $path
  if ($sig.Status -ne "Valid") { Fail "signature status $($sig.Status): $path" }
  $subject = $sig.SignerCertificate.Subject
  if (-not ($subject -eq "CN=$Publisher" -or $subject.StartsWith("CN=$Publisher, "))) {
    Fail "signed by '$subject', expected CN=$Publisher`: $path"
  }
  if (-not $sig.TimeStamperCertificate) { Fail "no timestamp: $path" }
  Write-Host ("ok  {0}  ({1})" -f $path, $subject)
}

function Verify-Installer {
  if (-not $Installer -or -not (Test-Path $Installer)) { Fail "-Installer must point at the setup .exe" }
  $signtool = Find-Signtool
  $setup = (Resolve-Path $Installer).Path
  Test-Signed $setup $signtool

  Write-Host "Installing silently to check the installed files"
  $p = Start-Process -FilePath $setup -ArgumentList "/S" -Wait -PassThru
  if ($p.ExitCode -ne 0) { Fail "silent install exited with $($p.ExitCode)" }
  $dir = Join-Path $env:LOCALAPPDATA "Programs\OpenBark"
  if (-not (Test-Path $dir)) { Fail "nothing installed at $dir" }
  $files = @(Get-ChildItem -Path $dir -Recurse -File -Include *.exe, *.dll)
  $names = $files | ForEach-Object { $_.Name }
  foreach ($must in "uninstall.exe", "sherpa-onnx-c-api.dll", "sherpa-onnx-cxx-api.dll", "onnxruntime.dll", "onnxruntime_providers_shared.dll") {
    if ($names -notcontains $must) { Fail "$must is missing from the installed files" }
  }
  if (@($files | Where-Object { $_.Extension -eq ".exe" -and $_.Name -ne "uninstall.exe" }).Count -lt 1) { Fail "the app's .exe is missing" }
  foreach ($f in $files) { Test-Signed $f.FullName $signtool }
  Write-Host "All $($files.Count) installed program files and the installer are signed by $Publisher, with timestamps."
}

switch ($Mode) {
  "Setup" { if ($Kind -eq "Artifact") { Setup-Artifact } else { Setup-SelfSigned } }
  "Verify" { Verify-Installer }
}
