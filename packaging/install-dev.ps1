#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Builds Canape, installs the service (CanapeService, LocalSystem), packs and
  signs the sparse identity package, and registers it so the FSE can use
  Canape as its home app.

.DESCRIPTION
  Developer setup only: Developer Mode must be on (the SCCD uses the test
  catalog). Run from the repository root in an elevated PowerShell:
    powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1
#>
param(
    [string]$InstallDir = "C:\Program Files\Canape",
    [string]$CertDir = "$HOME\dev\canape-cert",
    # Smart App Control can block fresh release build scripts; debug ones
    # usually pass, so this allows testing with a debug build.
    [ValidateSet("release", "debug")]
    [string]$BuildProfile = "release",
    [switch]$SetAsHomeApp
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$publisher = "CN=Canape Dev"

function Find-SdkTool([string]$name) {
    $tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\$name" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $tool) { throw "$name not found: install the Windows SDK (Visual Studio Build Tools)." }
    $tool.FullName
}

$devMode = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" -ErrorAction SilentlyContinue
if ($devMode.AllowDevelopmentWithoutDevLicense -ne 1) {
    throw "Developer Mode is off: Settings > System > Advanced > For developers."
}

Write-Host "1/6 Building ($BuildProfile)"
$cargoProfile = if ($BuildProfile -eq "release") { @("--release") } else { @() }
cargo build @cargoProfile -p canape-shell -p canape-service -p canape-ctl --manifest-path "$repo\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

Write-Host "2/6 Installing files to $InstallDir"
$service = Join-Path $InstallDir "canape-service.exe"
if (Test-Path $service) {
    # The running service locks its executable. Uninstalling a service that is
    # not registered writes to stderr, which PowerShell 5.1 turns into an error.
    try { & $service uninstall 2>&1 | Out-Null } catch { }
}
Get-Process canape-shell -ErrorAction SilentlyContinue | Stop-Process -Force
New-Item -ItemType Directory -Force $InstallDir | Out-Null
foreach ($exe in "canape-shell.exe", "canape-service.exe", "canape-ctl.exe") {
    Copy-Item "$repo\target\$BuildProfile\$exe" $InstallDir -Force
}
Copy-Item "$repo\packaging\identity\CustomCapability.SCCD" $InstallDir -Force

Write-Host "3/6 Installing the service"
& $service install
if ($LASTEXITCODE -ne 0) { throw "service install failed (see $env:ProgramData\Canape\service.log)" }

Write-Host "4/6 Development certificate"
New-Item -ItemType Directory -Force $CertDir | Out-Null
$pfx = Join-Path $CertDir "canape-dev.pfx"
$pfxPassword = "canape-dev"
if (-not (Test-Path $pfx)) {
    $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature `
        -FriendlyName "Canape Dev" -CertStoreLocation "Cert:\CurrentUser\My" `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
    $secure = ConvertTo-SecureString $pfxPassword -AsPlainText -Force
    Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $secure | Out-Null
    $cer = Join-Path $CertDir "canape-dev.cer"
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    Import-Certificate -FilePath $cer -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" | Out-Null
}

Write-Host "5/6 Packing and signing the identity package"
$msix = "$repo\target\Canape.Identity.msix"
& (Find-SdkTool "makeappx.exe") pack /d "$repo\packaging\identity" /p $msix /nv /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed" }
& (Find-SdkTool "signtool.exe") sign /fd SHA256 /f $pfx /p $pfxPassword $msix
if ($LASTEXITCODE -ne 0) { throw "signtool failed" }

Write-Host "6/6 Registering the package"
Get-AppxPackage -Name Canape | Remove-AppxPackage
Add-AppxPackage -Path $msix -ExternalLocation $InstallDir
$package = Get-AppxPackage -Name Canape
$aumid = "$($package.PackageFamilyName)!App"
Write-Host "Registered $aumid"

if ($SetAsHomeApp) {
    # Elevated PowerShell runs as the same user, so HKCU is the right hive.
    $key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration"
    # New-Item -Force would wipe the key's other values (StartupToGamingHome).
    if (-not (Test-Path $key)) { New-Item $key | Out-Null }
    Set-ItemProperty $key -Name GamingHomeApp -Value $aumid
    Write-Host "GamingHomeApp = $aumid"
}

Write-Host ""
Write-Host "Test launch:  explorer.exe shell:AppsFolder\$aumid"
Write-Host "Service:      & '$InstallDir\canape-ctl.exe' watch"
Write-Host "Logs:         $env:LOCALAPPDATA\Canape\shell.log, $env:ProgramData\Canape\service.log"
Write-Host "Home app:     Settings > Gaming > Full screen experience (or rerun with -SetAsHomeApp)"
