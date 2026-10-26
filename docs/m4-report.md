# M2.5 / M4 report: persistence, conflicts, ADL, auto TDP, light shell

Tested on the ROG Ally RC71L on 2026-10-21. Starting commit: `94a4db0`. **Release** build compiled locally, after Pierre turned Smart App Control off (`VerifiedAndReputablePolicyState = 0`, no reboot needed).

## What works

| Step | Result |
|---|---|
| `cargo test`, clippy, fmt under Windows | OK after one clippy fix (see below) |
| GitHub Actions CI | Green on `8ba2687` (linux 6 min, windows 20 min). Not used: the local release build was enough once SAC was off |
| `install-dev.ps1` (release) | OK after two fixes to the script (see below) |
| `service.log` | `device = ROG Ally`, `conflicting programs: [<name>]`; `[]` while that program is paused, back to `[<name>]` on restart |
| Yellow warning in the shell | Visible (checked by Pierre) |
| Logging of writes | `SetTdp { watts: 12 } -> Ok`, `SetAutoTdp { … } -> Ok` |
| Persistence | 12 W → `state.json`. Turbo set behind Canapé's back (direct DEVS) drops the limit, then `Restart-Service` logs `restored Saved { mode: Some(Manual), tdp_watts: Some(12), … }` and the limit comes back (table below) |
| GPU activity (PMLog) from the service | OK in session 0: `gpu_util` 0.0 at rest, 0.17–0.28 on light WebGL, 0.44–0.59 on heavy WebGL; matches the session 1 probe value for value |
| FPS (FrameMetrics) from the service | **Only once a user-session process has called `FrameMetrics_Start`** (see ADL) |
| Auto TDP, light load | Goes down by 1 W per step from 12 to 7 W (the floor) in 16 s, then stays there with no oscillation, 120 FPS |
| Auto TDP, heavy load | Rises quickly (7 → 21 W), then **oscillates** between 16 and 24 W (see Problems) |

### Persistence, load measurement (`% Processor Performance`, all cores)

| State | Samples |
|---|---|
| 12 W set by Canapé | 67–68 |
| Turbo behind Canapé's back (state.json still 12 W) | 116–123 |
| After `Restart-Service CanapeService` (restore) | 46 (stable) |

The two 12 W measurements differ, probably because of the firmware's boost windows. The restore is clear (≈120 → 46), but we can't derive an exact watts → performance mapping from it.

## ADL from session 0: precise diagnosis

Adapter list seen from the service (5 entries, all the iGPU "AMD Radeon Graphics", vendor 1002, ASIC `0x22`/valid `0xaf`, so integrated):

| | Service (session 0, LocalSystem) | Probe in the user session (1) |
|---|---|---|
| `atiadlxx.dll`, `ADL2_Main_Control_Create`, `NumberOfAdapters_Get` (5), `AdapterInfo_Get` | OK | OK |
| `present` | **0 on all 5** (no display in session 0) | 1 on all 5 |
| `ADL2_Adapter_FrameMetrics_Start(adapter 0, source 0)` | **-1** (ADL_ERR) at service startup | 0 |
| `ADL2_Adapter_FrameMetrics_Get` | No value (`fps` stays `null`), until a session 1 process calls `FrameMetrics_Start` | 0, ≈120 FPS on WebGL Aquarium (120 Hz screen) |
| `ADL2_New_QueryPMLogData_Get`, sensor 19 (GFX activity) | **OK** | OK, same values |

Notable observation: with the Aquarium already running, the service got no FPS (21:20:04). It got 120 FPS one second later, once the probe had called `FrameMetrics_Start` in session 1, **and kept getting them after the probe exited**. A service started after that probe even gets `FrameMetrics_Start = 0`. **[I]** The FrameMetrics start is global to the driver, but only takes effect when requested from a session that has a display.

Suggestion for M4: the shell (session 1) calls `ADL2_Adapter_FrameMetrics_Start` at startup (one call, then nothing). Or it measures the FPS itself and sends them to the service. GPU activity can stay on the service side.

## What I fixed

- **Clippy** (`canape-service/src/server.rs`): `for tick in 0u32..` is rejected ("for loop on unbounded range") by clippy 1.99, locally and in CI (which failed on `94a4db0`). Replaced with a wrapping counter; the first conflict scan still happens on the first tick. Commit `8ba2687`.
- **ADL** (`canape-hw/src/adl.rs`, `windows.rs`, `lib.rs`, `canape-service/src/winsvc.rs`):
  - `Adl::open()` returns the failing step and its code. The new `Device::diagnostics()` (empty by default) lets the service log it at startup (`adl: …`).
  - The adapter is chosen among the integrated AMD adapters that `exist`, preferring a `present` one. In session 0 none is `present`, and the old filter rejected everything.
  - A `FrameMetrics_Start` failure is no longer fatal: GPU activity still goes up, and `fps` stays `None`.
- **`install-dev.ps1`**:
  - `Stop-Process` returns before the exe is unlocked, so the copy failed after the service had already been uninstalled. It now waits for the exit (`Wait-Process`).
  - The release path was broken (my M2 change): in PowerShell 5.1, `if (…) { @("--release") }` returns a string, and splatting passes it character by character (`cargo build - - r e …`). The array is now built explicitly.

## Memory and CPU (release)

| Process | Private working set | Working set | Private bytes | Idle CPU (20 s) |
|---|---|---|---|---|
| `canape-service` | **1.4 MB** | 10.2 MB | 1.9 MB | 0 % |
| `canape-shell` (software rendering, direct XInput) | **10.9 MB** | 29.2 MB | 12.8 MB | 0.94 % of one core |

Targets: service < 10 MB, OK; shell < 80 MB, **OK** (115 MB in M2, debug + OpenGL). The shell's idle CPU (foreground, gamepad polled) has halved compared with M2 (2 %), but isn't ≈ 0 yet.

## Conflicts

Same as M2: another tuning tool (service and watchdog), ASUS Optimization / System Analysis / Diagnosis, ROG Live Service, RTSS. Detection only covers that tool (the other ASUS services are not on the list, and that's fine: they don't write the PPT limits, as far as we know).

## Problems found

- **Auto TDP oscillates under a GPU/CPU-heavy load.** WebGL Aquarium with 30,000 fish, target 60 FPS: a sawtooth between 16 and 24 W, period 20–40 s, FPS between 55 and 71. The TDP keeps going down while FPS are already below 60 (18 → 16 W at 58.4 → 56.6 FPS). Cause: the rule `fps >= low && gpu_has_slack` (`canape-autotdp`) lowers as long as GPU activity is < 80 %. But on the Ally the GFX activity reads only ≈ 50 % even when FPS depend on the TDP (load partly CPU-limited). So the controller always sees slack, lowers down to the deficit, raises again by 2–4 W, and starts over. Ideas: apply the slack rule only when FPS sit at their cap (≥ target, stable); remember the last wattage that held the target and don't go below it for a while; a longer `cooldown_down`. Data in the log of this session. Not fixed: it's an algorithm design choice.
- No real game tested (Pierre had none). Tests were done with WebGL Aquarium in Edge (120 Hz, vsync). FPS weren't compared with RTSS/Game Bar, but with the session 1 probe (identical values).
- Side effect: at the end of the test, auto TDP is off and the limit stays at **17 W manual** (last value from the controller) until the next mode change.

## Still open

- FPS on the service side: arm FrameMetrics from the shell (or measure on the shell side).
- Auto TDP oscillation (above).
- The shell's idle CPU (0.94 % of one core).
- FSE boot not retested (nothing changed on the startup side).

## Retest 13ee476

2026-10-21, local release build. `cargo test` (9 + 8 + … tests), clippy and fmt OK under Windows, no fix needed. The other tuning tool was paused during the measurements and restarted at the end (Pierre's approval).

### FPS from the service

- `shell.log`: `adl: frame metrics started on adapter 0`. Starting the shell is enough, with no probe.
- `service.log` at startup: `adl: frame metrics started on adapter 0` as well. **[I]** The arming done by the shell earlier persists at the driver level.
- `canape-ctl`: `fps` ≈ 120 on light Aquarium, 60–70 on heavy. **OK, no probe.**

### Auto TDP, heavy load (Aquarium 30,000 fish, target 60, 5 min)

| | 94a4db0 | 13ee476 |
|---|---|---|
| After stabilization (4 min) | sawtooth 16–24 W | **18 W for 87 % of the time** (104/120 samples); 16–17 W ×5, 19–20 W ×5, 24 W ×6 |
| Changes | ~30 in 80 s | **13 in 5 min** (4 at startup: 12 → 19 W in 6 s) |
| FPS | 55–71 | average 60.6; 5 samples < 57 (the probes) |

- Downward probes come every ~2 min (18 → 17 → 16 W, back to 18 W within 2–4 s), as intended.
- A one-off stall at 20.8 FPS (21:39:11, probably on Edge's side) caused a jump to 24 W, 20 → 24 W in one step.
- Minor: two probes went down while FPS were already below 98 % of the target (18 → 17 W at 58.4 FPS, 17 → 16 W at 58.1). Maybe the smoothed FPS was still above the threshold.

### Auto TDP, light load (default Aquarium) right after the heavy one: regression

- **The TDP stays stuck at 24 W for about 4 min** (21:39:11 → 21:43:19) at 120 FPS and 18–19 % GPU. That's the floor created by the rise after the stall, whose hold time had doubled after the failed probes (≈ 240 ticks).
- Once released, it drops to 7 W in 48 s (1 W every 2–4 s) and stays there.
- Before this fix, the drop to 7 W took 16 s. Suggestion: release the floor right away when FPS sit well above the target (e.g. > 110 %) with low GPU activity, which is a clear change of load; and/or not let a rise caused by a one-off stall (a single sample, very low FPS) become a floor.

### Idle CPU and memory (release, shell in the foreground)

| Process | Private working set | Idle CPU (30 s) |
|---|---|---|
| `canape-shell` | 11.3 MB | **0.31 %** of one core (0.94 % before, 2 % in M2) |
| `canape-service` | 1.3 MB | 0 % |

### End state

Auto TDP off, `set_performance_mode performance` (firmware balanced, `tdp_watts: null`). `state.json` = `performance`. The other tool restarted. Note: earlier in the evening Pierre had turned on auto TDP and 23 W from the shell (`SetTdp` 18 → 23 then `SetAutoTdp enabled: true` at 21:27, restored at startup); it's off now.

### Tooling for M3 (Game Bar widget), read-only

- **Visual Studio Build Tools 2022 17.14** (`C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`). Workloads: `VCTools` and `MSBuildTools` only.
- **No** `Microsoft.VisualStudio.Workload.Universal`, no `ComponentGroup.UWP.BuildTools`, no `ManagedDesktop`, no XAML/.NET Native targets for UWP.
- **.NET: runtimes only** (NETCore 8.0.21 / 8.0.32 / 10.0.9, WindowsDesktop 8.0.32 / 10.0.9), **no SDK** (`dotnet --list-sdks` is empty).
- Windows SDK 10.0.26100.0 present (`Platforms\UAP\10.0.26100.0`).
- So to build a UWP widget on the Ally, you'd need to install the UWP workload (or the .NET SDK, depending on the chosen approach): that's Pierre's call.
