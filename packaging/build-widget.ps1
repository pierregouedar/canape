<#
.SYNOPSIS
  Builds, signs and installs the Game Bar widget (widget\CanapeWidget).

.DESCRIPTION
  Needs the Visual Studio Build Tools workload
  Microsoft.VisualStudio.Workload.UniversalBuildTools (see widget\README.md)
  and the development certificate created by install-dev.ps1. Run from the
  repository root (no elevation needed):
    powershell -ExecutionPolicy Bypass -File packaging\build-widget.ps1
#>
param(
    [string]$CertDir = "$HOME\dev\canape-cert",
    [ValidateSet("Release", "Debug")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$pfx = Join-Path $CertDir "canape-dev.pfx"
if (-not (Test-Path $pfx)) { throw "$pfx missing: run packaging\install-dev.ps1 first." }

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find "MSBuild\**\Bin\MSBuild.exe" | Select-Object -First 1
if (-not $msbuild) { throw "MSBuild not found: install the Visual Studio Build Tools." }

$out = "$repo\target\widget\"
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
$arguments = @(
    "$repo\widget\CanapeWidget\CanapeWidget.csproj",
    "/restore",
    "/p:Configuration=$Configuration",
    "/p:Platform=x64",
    "/p:UapAppxPackageBuildMode=SideloadOnly",
    "/p:AppxBundle=Never",
    "/p:AppxPackageDir=$out",
    "/p:AppxPackageSigningEnabled=true",
    "/p:PackageCertificateKeyFile=$pfx",
    "/p:PackageCertificatePassword=canape-dev"
)
& $msbuild @arguments
if ($LASTEXITCODE -ne 0) { throw "msbuild failed" }

$package = Get-ChildItem $out -Recurse -Include *.msix, *.appx |
    Where-Object { $_.FullName -notmatch "\\Dependencies\\" } | Select-Object -First 1
# Only the frameworks that are missing or older: reinstalling one that Game Bar,
# the Xbox app or other widgets are using fails with 0x80073D02.
Add-Type -AssemblyName System.IO.Compression.FileSystem
function Get-PackageIdentity([string]$path) {
    $zip = [IO.Compression.ZipFile]::OpenRead($path)
    try {
        $reader = New-Object IO.StreamReader($zip.GetEntry("AppxManifest.xml").Open())
        ([xml]$reader.ReadToEnd()).Package.Identity
    } finally { $zip.Dispose() }
}
$dependencies = @(Get-ChildItem $out -Recurse -Include *.appx, *.msix |
    Where-Object { $_.FullName -match "\\Dependencies\\x64\\" } |
    Where-Object {
        $identity = Get-PackageIdentity $_.FullName
        $installed = Get-AppxPackage -Name $identity.Name | Where-Object Architecture -eq "X64" |
            Where-Object { [version]$_.Version -ge [version]$identity.Version }
        -not $installed
    } | ForEach-Object FullName)
# Same version, different content fails with 0x80073CFB: remove the old one.
Get-AppxPackage -Name Canape.Widget | Remove-AppxPackage
Write-Host "Installing $($package.FullName)"
if ($dependencies.Count -gt 0) {
    Add-AppxPackage -Path $package.FullName -DependencyPath $dependencies -ForceUpdateFromAnyVersion
} else {
    Add-AppxPackage -Path $package.FullName -ForceUpdateFromAnyVersion
}
Write-Host "Done: open Game Bar (Win+G or the Xbox button), Widgets menu, Canapé."
