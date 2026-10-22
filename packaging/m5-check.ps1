<#
.SYNOPSIS
  Automated part of the M5 checks on the Ally: telemetry, accent sharing,
  game profile, bench, memory. Writes a Markdown summary.

.DESCRIPTION
  Writes power limits through the service (bench sweep): run it only with
  the owner's approval, any other tool that writes the same firmware
  settings paused, and a game (or WebGL Aquarium in full screen) running so
  the frame rate is measured. Restores the power
  state and the accent it found. Does not need elevation.
    powershell -ExecutionPolicy Bypass -File packaging\m5-check.ps1 -Game msedge.exe
#>
param(
    # Executable of the game running during the checks (for the profile).
    [Parameter(Mandatory = $true)][string]$Game,
    [string]$InstallDir = "C:\Program Files\Canape",
    [string]$OutDir = "$HOME\dev\canape-m5",
    [switch]$SkipBench
)

$ErrorActionPreference = "Stop"
$ctl = Join-Path $InstallDir "canape-ctl.exe"
New-Item -ItemType Directory -Force $OutDir | Out-Null
$results = [System.Collections.Generic.List[string]]::new()

function Ctl($request) {
    $json = $request | ConvertTo-Json -Compress -Depth 6
    # Windows PowerShell 5.1 drops the double quotes of native arguments.
    $out = & $ctl ($json -replace '"', '\"') 2>&1
    if ($LASTEXITCODE -ne 0) { throw "canape-ctl $($request.type): $out" }
    $out | ConvertFrom-Json
}

function Status { Ctl @{ type = "get_status" } }

function Check([string]$name, [bool]$ok, [string]$detail) {
    $mark = if ($ok) { "OK" } else { "FAIL" }
    $results.Add("| $name | $mark | $detail |")
    Write-Host ("[{0}] {1} {2}" -f $mark, $name, $detail)
}

$before = Status
Write-Host "Initial status: mode $($before.performance_mode), TDP $($before.tdp_watts), accent $($before.accent)"

# 1. Telemetry.
$log = Get-Content "$env:ProgramData\Canape\service.log" -Tail 400 -ErrorAction SilentlyContinue
$device = ($log | Select-String "device = " | Select-Object -Last 1).Line
Check "Device detected" ($device -match "ROG Ally") "$device"
$s = Status
Check "Actual power draw (power_watts)" ($null -ne $s.power_watts) "$($s.power_watts) W for a $($s.tdp_watts) W limit"
Check "GPU activity" ($null -ne $s.gpu_util) "$($s.gpu_util)"
Check "FPS" ($null -ne $s.fps -and $s.fps -ge 5) "$($s.fps) FPS (a game must be running)"

# 2. Accent shared through the service.
$accent = $before.accent
Ctl @{ type = "set_accent"; color = "#14b8a6" } | Out-Null
Start-Sleep -Seconds 2
Check "Shared accent" ((Status).accent -eq "#14b8a6") "read: $((Status).accent) (check the shell and the widget by eye)"
if ($accent) { Ctl @{ type = "set_accent"; color = $accent } | Out-Null }

# 3. Game profile.
$existing = (Ctl @{ type = "list_profiles" }).profiles | Where-Object { $_.exe -ieq $Game }
$auto = $before.auto_tdp | Select-Object *
$auto.enabled = $true
$auto.target_fps = 40
Ctl @{ type = "save_profile"; profile = @{ exe = $Game; auto_tdp = $auto; learned_watts = $null } } | Out-Null
Ctl @{ type = "foreground_app"; exe = $Game } | Out-Null
$s = Status
Check "Profile activated" ($s.active_profile -ieq $Game -and $s.auto_tdp.target_fps -eq 40) "profile $($s.active_profile), target $($s.auto_tdp.target_fps)"
Start-Sleep -Seconds 40
$learned = ((Ctl @{ type = "list_profiles" }).profiles | Where-Object { $_.exe -ieq $Game }).learned_watts
Check "Learned TDP" ($null -ne $learned) "learned_watts = $learned (needs 30 s without a change)"

# 4. Bench.
if (-not $SkipBench) {
    $plan = @{ from_watts = 25; to_watts = 7; step_watts = 6; settle_secs = 3; measure_secs = 8 }
    Ctl @{ type = "start_bench"; plan = $plan } | Out-Null
    $deadline = (Get-Date).AddMinutes(3)
    do { Start-Sleep -Seconds 2; $s = Status } while ($s.bench -and (Get-Date) -lt $deadline)
    $report = (Ctl @{ type = "get_bench_report" }).report
    $points = ($report.points | ForEach-Object { "$($_.watts) W: $([math]::Round($_.fps_avg)) FPS (min $([math]::Round($_.fps_low)))" }) -join ", "
    Check "Bench completed" ($report.completed -and $report.points.Count -ge 3) $points
    Check "State restored after the bench" ($null -eq $s.bench -and $s.active_profile -ieq $Game) "TDP $($s.tdp_watts) W, profile $($s.active_profile)"
    $report | ConvertTo-Json -Depth 6 | Set-Content "$OutDir\bench.json" -Encoding UTF8
}

# 5. Memory.
foreach ($name in "canape-service", "canape-shell", "CanapeWidget") {
    $p = Get-Process $name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($p) {
        Check "Memory $name" $true ("{0:N1} MB private, {1:N1} MB working set" -f ($p.PrivateMemorySize64 / 1MB), ($p.WorkingSet64 / 1MB))
    } else {
        Check "Memory $name" $false "process not running"
    }
}

# Restore what was there before.
# Back to what the shell reported: it only resends on a change.
Ctl @{ type = "foreground_app"; exe = $before.foreground } | Out-Null
if (-not $existing) { Ctl @{ type = "delete_profile"; exe = $Game } | Out-Null }
if ($before.performance_mode -eq "manual" -and $before.tdp_watts) {
    Ctl @{ type = "set_tdp"; watts = $before.tdp_watts } | Out-Null
} elseif ($before.performance_mode) {
    Ctl @{ type = "set_performance_mode"; mode = $before.performance_mode } | Out-Null
}
Ctl @{ type = "set_auto_tdp"; settings = $before.auto_tdp } | Out-Null
Get-Content "$env:ProgramData\Canape\service.log" -Tail 60 | Set-Content "$OutDir\service-tail.log" -Encoding UTF8

$summary = @("| Check | Result | Detail |", "|---|---|---|") + $results
$summary | Set-Content "$OutDir\results.md" -Encoding UTF8
Write-Host ""
Write-Host "Summary: $OutDir\results.md (bench.json, service-tail.log next to it)"
