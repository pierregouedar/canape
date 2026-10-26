# M3 report: Game Bar widget

Tested on the ROG Ally RC71L on 2026-10-21. Starting commit: `2ccedd6`.

## Tooling

- Added to VS Build Tools 2022 17.14 with Pierre's approval: workload `Microsoft.VisualStudio.Workload.UniversalBuildTools` (+ recommended). Installed SDK: **10.0.26100.0** only.
- The README command failed: `setup.exe modify` doesn't accept `--wait` (exit 87, "L'option 'wait' est inconnue"); only the `vs_buildtools.exe` bootstrapper takes it. Fixed to `--quiet --norestart`.

## What works

| Step | Result |
|---|---|
| `packaging\build-widget.ps1` | Builds (.NET Native, Release x64), signs (`CN=Canape Dev`) and installs **after 3 fixes** (below) |
| Package | `Canape.Widget_0.1.0.0_x64__84zrj1c9kn010`, `Status Ok`, `SignatureKind Developer` |
| Game Bar catalog | `AppExtensionCatalog.Open("microsoft.gameBarUIExtension")` lists `CanapeWidget / Canapé`, next to another tool's HUD, ASUS Command Center and the Xbox widgets |
| Game Bar (Win+G) | Listed in the Widgets menu, opens, **connected to the service** (live values). Checked by Pierre |
| Pipe from the AppContainer | `CreateFileFromAppW` OK with the current DACL (`AC` = ALL APPLICATION PACKAGES): **no Win32 error, no need for `WD`** |
| Actions (checked by Pierre, confirmed by `service.log`) | Modes (`SetPerformanceMode Silent / Turbo / Performance`), ±1 W (`SetTdp` 14 → 10, one line per press), auto TDP (`SetAutoTdp enabled: true, target_fps: 40`, then 60), then Performance mode, which turns auto TDP off |
| Consistency with the shell | The shell shows the same state as the widget (checked by Pierre) |

## What I fixed

- **`TargetPlatformVersion` 10.0.19041.0 → 10.0.26100.0** (`CanapeWidget.csproj`). With the 19041 SDK missing, no WinRT type resolved (`CS0234 Windows.ApplicationModel`, `Windows.Data`…). `TargetPlatformMinVersion` stays 19041.
- **`mp:PhoneIdentity`** added to `Package.appxmanifest` (error `APPX1673`, required by the UWP packaging targets even off phones). Own GUID, null publisher.
- **`build-widget.ps1`**: `Add-AppxPackage -DependencyPath … -ForceUpdateFromAnyVersion` also forced the frameworks (VCLibs 14.0.33519.0, .NET Native Framework 2.2.29512.0 / Runtime 2.2.28604.0), which were already installed at those exact versions. It failed with **0x80073D02** because they're in use by Game Bar, the Xbox app, another tool's HUD, Gaming Services, Realtek… The script now reads the identity of each bundled dependency and only passes those that are missing or older.
- `widget/README.md`: workload command (`--wait` removed).

## Memory and CPU (widget open in Game Bar)

| Process | Private working set | Working set | Private bytes | CPU (20 s) |
|---|---|---|---|---|
| `CanapeWidget` | **24.3 MB** | 84.3 MB | 36.8 MB | 0.55 % of one core |
| `canape-shell` (for reference) | 11.2 MB | 31.6 MB | 13.1 MB | |
| `canape-service` | 1.4 MB | 10.4 MB | 2.0 MB | |

## Problems found

- **Style**: Pierre finds the look of the widget, and of the shell, poor. "Not the priority, but we'll need to improve it a lot."
- **±1 W from a firmware mode**: with no known TDP (`tdp_watts: null` in Silent/Performance/Turbo), the widget starts from an arbitrary 15 W. The first "−1" from Turbo therefore sent 14 W. Better: disable ±1 in a firmware mode, or start from the mode's nominal value once it's known.
- Not tested: the Xbox button in the FSE (the widget was tested through Win+G on the desktop). Pierre's home app is another one.

## Still open

- The UI style of the widget and the shell.
- The ±1 W behavior in a firmware mode.
- Tightening the pipe DACL: today `AC` (all AppContainer apps); ideally the widget package's SID only.
- Opening the widget from the FSE (Xbox button) with Canapé as home app.
