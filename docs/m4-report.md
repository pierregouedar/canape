# M2.5 / M4 report: persistence, conflicts, ADL, auto TDP, light shell

Tested on the ROG Ally RC71L on 2026-10-21. Starting commit: `c617cf3`. **Release** build compiled locally, after Pierre turned Smart App Control off (`VerifiedAndReputablePolicyState = 0`, no reboot needed).

## What works

| Step | Result |
|---|---|
| `cargo test`, clippy, fmt under Windows | OK after one clippy fix (see below) |
| GitHub Actions CI | Green on `a429a96` (linux 6 min, windows 20 min). Not used: the local release build was enough once SAC was off |
| `install-dev.ps1` (release) | OK after two fixes to the script (see below) |
| `service.log` | `device = ROG Ally`, `conflicting programs: ["Winhanced"]`; `[]` while Winhanced is paused, back to `["Winhanced"]` on restart |
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

- **Clippy** (`canape-service/src/server.rs`): `for tick in 0u32..` is rejected ("for loop on unbounded range") by clippy 1.99, locally and in CI (which failed on `c617cf3`). Replaced with a wrapping counter; the first conflict scan still happens on the first tick. Commit `a429a96`.
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


## Problems found

- **Auto TDP oscillates under a GPU/CPU-heavy load.** WebGL Aquarium with 30,000 fish, target 60 FPS: a sawtooth between 16 and 24 W, period 20–40 s, FPS between 55 and 71. The TDP keeps going down while FPS are already below 60 (18 → 16 W at 58.4 → 56.6 FPS). Cause: the rule `fps >= low && gpu_has_slack` (`canape-autotdp`) lowers as long as GPU activity is < 80 %. But on the Ally the GFX activity reads only ≈ 50 % even when FPS depend on the TDP (load partly CPU-limited). So the controller always sees slack, lowers down to the deficit, raises again by 2–4 W, and starts over. Ideas: apply the slack rule only when FPS sit at their cap (≥ target, stable); remember the last wattage that held the target and don't go below it for a while; a longer `cooldown_down`. Data in the log of this session. Not fixed: it's an algorithm design choice.
- No real game tested (Pierre had none). Tests were done with WebGL Aquarium in Edge (120 Hz, vsync). FPS weren't compared with RTSS/Game Bar, but with the session 1 probe (identical values).
- Side effect: at the end of the test, auto TDP is off and the limit stays at **17 W manual** (last value from the controller) until the next mode change.

## Still open

- FPS on the service side: arm FrameMetrics from the shell (or measure on the shell side).
- Auto TDP oscillation (above).
- The shell's idle CPU (0.94 % of one core).
- FSE boot not retested (nothing changed on the startup side).
