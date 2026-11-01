#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Installs Canape from a release folder (no Rust, no Windows SDK needed):
  copies the programs, installs the service, trusts the release certificate,
  registers the identity package and installs the Game Bar widget.

.DESCRIPTION
  Run from the extracted release, in an elevated PowerShell (or double-click
  Install-Canape.cmd). Developer Mode must be on: the identity package that
  lets the full-screen experience use Canape as its home app is unlocked by a
  test capability. Uninstall with uninstall-dev.ps1 from the same folder.
    powershell -ExecutionPolicy Bypass -File install-release.ps1 [-SetAsHomeApp] [-NoWidget]
#>
param(
    [string]$InstallDir = "C:\Program Files\Canape",
    [switch]$SetAsHomeApp,
    [switch]$NoWidget
)

$ErrorActionPreference = "Stop"
$here = $PSScriptRoot

function Require([string]$name) {
    $path = Join-Path $here $name
    if (-not (Test-Path $path)) { throw "$name is missing from the release folder ($here)." }
    $path
}

$devMode = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" -ErrorAction SilentlyContinue
if ($devMode.AllowDevelopmentWithoutDevLicense -ne 1) {
    throw "Developer Mode is off: Settings > System > Advanced > For developers."
}

$sums = Join-Path $here "SHA256SUMS.txt"
if (Test-Path $sums) {
    Write-Host "1/6 Checking the files"
    foreach ($line in Get-Content $sums) {
        if ($line -notmatch '^([0-9a-fA-F]{64})\s+\*?(.+)$') { continue }
        $file = Join-Path $here $Matches[2]
        if (-not (Test-Path $file)) { continue }
        if ((Get-FileHash $file -Algorithm SHA256).Hash -ne $Matches[1].ToUpper()) {
            throw "$($Matches[2]) does not match SHA256SUMS.txt: download the release again."
        }
    }
} else {
    Write-Warning "SHA256SUMS.txt not found: the files are not checked."
}

Write-Host "2/6 Installing the files to $InstallDir"
$service = Join-Path $InstallDir "canape-service.exe"
if (Test-Path $service) {
    try { & $service uninstall 2>&1 | Out-Null } catch { }
}
Get-Process canape-shell, canape-shell-gpu -ErrorAction SilentlyContinue | Stop-Process -Force -PassThru | Wait-Process -Timeout 10
New-Item -ItemType Directory -Force $InstallDir | Out-Null
foreach ($name in "canape-shell.exe", "canape-shell-gpu.exe", "canape-service.exe", "canape-ctl.exe", "CustomCapability.SCCD") {
    Copy-Item (Require $name) $InstallDir -Force
}

Write-Host "3/6 Installing the service"
& $service install
if ($LASTEXITCODE -ne 0) { throw "service install failed (see $env:ProgramData\Canape\service.log)" }

Write-Host "4/6 Trusting the release certificate"
Import-Certificate -FilePath (Require "canape-release.cer") -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" | Out-Null

Write-Host "5/6 Registering the identity package"
Get-AppxPackage -Name Canape | Remove-AppxPackage
Add-AppxPackage -Path (Require "Canape.Identity.msix") -ExternalLocation $InstallDir
$package = Get-AppxPackage -Name Canape
$aumid = "$($package.PackageFamilyName)!App"
Write-Host "Registered $aumid"

if (-not $NoWidget) {
    Write-Host "6/6 Installing the Game Bar widget"
    $widgetDir = Join-Path $here "widget"
    $widget = Get-ChildItem $widgetDir -Filter "*.msix*" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $widget) { $widget = Get-ChildItem $widgetDir -Filter "*.appx" -ErrorAction SilentlyContinue | Select-Object -First 1 }
    if ($widget) {
        # Frameworks already installed (Game Bar uses some) are left alone:
        # reinstalling one in use fails with 0x80073D02.
        $dependencies = @(Get-ChildItem (Join-Path $widgetDir "Dependencies") -Recurse -Include *.appx, *.msix -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match "x64" } | ForEach-Object FullName)
        Get-AppxPackage -Name Canape.Widget | Remove-AppxPackage
        try {
            if ($dependencies.Count -gt 0) {
                Add-AppxPackage -Path $widget.FullName -DependencyPath $dependencies -ForceUpdateFromAnyVersion
            } else {
                Add-AppxPackage -Path $widget.FullName -ForceUpdateFromAnyVersion
            }
        } catch {
            Write-Warning "The widget was not installed: $($_.Exception.Message)"
        }
    } else {
        Write-Warning "No widget package in the release folder."
    }
} else {
    Write-Host "6/6 Widget skipped"
}

if ($SetAsHomeApp) {
    $key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration"
    if (-not (Test-Path $key)) { New-Item $key | Out-Null }
    Set-ItemProperty $key -Name GamingHomeApp -Value $aumid
    Write-Host "GamingHomeApp = $aumid"
}

Write-Host ""
Write-Host "Done. Test launch: explorer.exe shell:AppsFolder\$aumid"
Write-Host "Home app: Settings > Gaming > Full screen experience (or rerun with -SetAsHomeApp)."
Write-Host "Uninstall: powershell -ExecutionPolicy Bypass -File uninstall-dev.ps1"
