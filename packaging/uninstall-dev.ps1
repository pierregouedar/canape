#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Removes what install-dev.ps1 and build-widget.ps1 installed: the service,
  the identity package, the widget package, the home app setting and the
  installed binaries.

.DESCRIPTION
  Before removing the service, asks it to put the hardware back to neutral
  settings: firmware fan curves, no frame rate limit, controller in gamepad
  mode, back buttons on their own keys, stick rings left to the controller,
  and no charge limit if one was set through Canape. Once the service is
  gone, nothing would undo them.

  Also asks it to revert the Windows tweaks Canape applied (each value put
  back as it was), and lists the preinstalled apps removed from the
  Windows tweaks page: those must be installed again from the Microsoft
  Store by hand.

  Keeps the user data (settings, profiles, library cache, logs, downloaded
  tools) unless -RemoveData is given, and the development certificate unless
  -RemoveCertificate is given. Games installed from Canape stay on disk.
  Run from an elevated PowerShell:
    powershell -ExecutionPolicy Bypass -File packaging\uninstall-dev.ps1
#>
param(
    [string]$InstallDir = "C:\Program Files\Canape",
    [string]$CertDir = "$HOME\dev\canape-cert",
    # Also deletes %ProgramData%\Canape and %LOCALAPPDATA%\Canape.
    [switch]$RemoveData,
    # Also removes the "CN=Canape Dev" certificate and $CertDir.
    [switch]$RemoveCertificate
)

$ErrorActionPreference = "Stop"
$serviceName = "CanapeService"
$publisher = "CN=Canape Dev"
$service = Join-Path $InstallDir "canape-service.exe"
$ctl = Join-Path $InstallDir "canape-ctl.exe"

# Windows PowerShell 5.1 (and PowerShell 7 before 7.3) drop the double
# quotes of native arguments: they must be escaped.
$legacyArguments = -not (Get-Variable PSNativeCommandArgumentPassing -ErrorAction SilentlyContinue) -or
    $PSNativeCommandArgumentPassing -eq "Legacy"

# Returns the decoded response, or $null when the service cannot be reached
# or answers with an error (reported as a warning).
function Ctl($request) {
    $json = $request | ConvertTo-Json -Compress -Depth 6
    if ($legacyArguments) { $json = $json -replace '"', '\"' }
    $ErrorActionPreference = "Continue"
    $out = & $ctl $json 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($code -ne 0) {
        Write-Warning "$($request.type): $out"
        return $null
    }
    $out | ConvertFrom-Json
}

Write-Host "1/6 Resetting the hardware settings"
$running = Get-Service $serviceName -ErrorAction SilentlyContinue | Where-Object Status -eq "Running"
if ($running -and (Test-Path $ctl)) {
    $status = Ctl @{ type = "get_status" }
    Ctl @{ type = "reset_fan_curves" } | Out-Null
    Ctl @{ type = "set_fps_limit"; fps = $null } | Out-Null
    Ctl @{ type = "set_controller_mode"; mode = "gamepad" } | Out-Null
    Ctl @{ type = "reset_back_buttons" } | Out-Null
    Ctl @{ type = "set_lighting"; lighting = $null } | Out-Null
    if ($status -and $null -ne $status.battery_charge_limit -and $status.battery_charge_limit -lt 100) {
        Ctl @{ type = "set_battery_charge_limit"; percent = 100 } | Out-Null
    }
    # Every Windows tweak Canape applied, back to what it replaced.
    Ctl @{ type = "revert_tweaks" } | Out-Null
} else {
    Write-Warning "The service is not running: fans, frame rate limit, controller mode, back buttons, stick rings, charge limit and Windows tweaks stay as they are."
}

# Apps removed from the Windows tweaks page (shell.json, per user).
$shellJson = "$env:LOCALAPPDATA\Canape\shell.json"
if (Test-Path $shellJson) {
    $removed = @((Get-Content $shellJson -Raw | ConvertFrom-Json).removed_apps | Where-Object { $_ })
    if ($removed.Count -gt 0) {
        Write-Host "Apps removed through Canape (install them again from the Microsoft Store if needed):"
        foreach ($name in $removed) { Write-Host "  $name" }
    }
}

Write-Host "2/6 Removing the service"
# Stop-Process returns before the exe is unlocked: wait for the exit.
Get-Process canape-shell -ErrorAction SilentlyContinue | Stop-Process -Force -PassThru | Wait-Process -Timeout 10
if (Get-Service $serviceName -ErrorAction SilentlyContinue) {
    if (Test-Path $service) {
        & $service uninstall
        if ($LASTEXITCODE -ne 0) { throw "service uninstall failed" }
    } else {
        Stop-Service $serviceName -Force -ErrorAction SilentlyContinue
        & sc.exe delete $serviceName | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "sc.exe delete $serviceName failed" }
    }
}

Write-Host "3/6 Removing the packages"
$package = Get-AppxPackage -Name Canape
Get-AppxPackage -Name Canape | Remove-AppxPackage
Get-AppxPackage -Name Canape.Widget | Remove-AppxPackage

Write-Host "4/6 Home app setting"
# Per-user AUMID of the FSE home app (install-dev.ps1 -SetAsHomeApp writes
# it): only cleared when it still names the Canape package.
$key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration"
$homeApp = (Get-ItemProperty $key -Name GamingHomeApp -ErrorAction SilentlyContinue).GamingHomeApp
$family = if ($package) { $package.PackageFamilyName } else { "Canape_" }
if ($homeApp -and $homeApp.StartsWith($family, [StringComparison]::OrdinalIgnoreCase)) {
    Remove-ItemProperty $key -Name GamingHomeApp
    Write-Host "GamingHomeApp removed ($homeApp)"
}

Write-Host "5/6 Removing $InstallDir"
if (Test-Path $InstallDir) { Remove-Item -Recurse -Force $InstallDir }

Write-Host "6/6 Data and certificate"
$data = @("$env:ProgramData\Canape", "$env:LOCALAPPDATA\Canape")
if ($RemoveData) {
    foreach ($dir in $data) {
        if (Test-Path $dir) { Remove-Item -Recurse -Force $dir; Write-Host "Removed $dir" }
    }
} else {
    Write-Host "Kept $($data -join ', ') (-RemoveData deletes them)"
}
if ($RemoveCertificate) {
    Get-ChildItem Cert:\LocalMachine\TrustedPeople, Cert:\CurrentUser\My |
        Where-Object Subject -eq $publisher | Remove-Item
    if (Test-Path $CertDir) { Remove-Item -Recurse -Force $CertDir }
    Write-Host "Removed the $publisher certificate"
} else {
    Write-Host "Kept the $publisher certificate (-RemoveCertificate removes it)"
}

Write-Host ""
Write-Host "Done. Pick another home app in Settings > Gaming > Full screen experience if Canape was the one."
