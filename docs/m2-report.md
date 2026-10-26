# M2 report: service, ATKACPI backend, Slint shell

Tested on the ROG Ally on 2026-10-21 (Windows 11 10.0.26200, Rust 1.99.1 MSVC). Starting commit: `b0c4e79`.

## Model


## What works

| Step | Result |
|---|---|
| `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check` under Windows | OK, no compile or link fix needed |
| Release build | **Blocked by Smart App Control** (see below). Tests run with the **debug** build |
| `install-dev.ps1 -BuildProfile debug` (elevated) | OK: `CanapeService installed and started`, package re-registered |
| `CanapeService` | `Running`, `Automatic`; `service.log`: `device = ROG Ally` (not `mock`) |
| `canape-ctl watch` (read-only) | CPU temperature 39–64 °C depending on load, battery 100 % (matches `Win32_Battery`, on AC); one push per second **after the fix** |
| `set_performance_mode` silent, `set_tdp` 10 then 15 W | Firmware answers `ok` (DEVS = 1); status `manual` / 15 W |
| TDP effect (all-core load, Winhanced paused) | `% Processor Performance` while under load: ~125 % without a Canapé limit, **42 % at 10 W** (measured twice, same result), **66 % at 15 W**. The limits apply |
| Shell via `shell:AppsFolder\Canape_84zrj1c9kn010!App` | Full screen (window = screen, 1280×720 logical points), foreground |
| Single instance | 2nd launch: no 2nd process; `shell.log`: `already running: activation forwarded` |
| Gamepad (checked by Pierre) | D-pad and stick move between tiles, A confirms, mode and TDP applied through the service, live status |

## What I fixed

- **Status pushes blocked on Windows** (`crates/canape-service/src/server.rs`). Each client's pipe is a synchronous handle, and Windows serializes I/O on the same file object (`try_clone` duplicates the handle). While the `client` thread waits in `recv()`, the `tick` thread's `WriteFile` waits too, so `watch` got a single line. Fix: after `Subscribe`, the connection is push-only and the server stops reading it (documented on `Request::Subscribe`). Result: 5 pushes in 5 s. The Unix socket wasn't affected (full duplex), so the bug was invisible from Linux.
- **`install-dev.ps1 -BuildProfile debug`**: lets you install a debug build when SAC blocks release.
- `packaging/README.md`: Windows pitfalls (SAC, quoting `canape-ctl` arguments in PowerShell 5.1).

## Memory and CPU (debug build)

| Process | Private working set | Working set | Private bytes | Idle CPU |
|---|---|---|---|---|
| `canape-service` | **0.6 MB** | 6 MB | 0.9 MB | 0 % |
| `canape-shell` | **115 MB** | 190 MB | 278 MB | 2 % of one core (0.13 % of the machine) |

Targets (docs/architecture.md): service < 10 MB, OK; shell < 80 MB, **over target in debug**. Needs to be measured again in release. The shell's 2 % of one core at rest likely comes from a redraw or gamepad polling loop; worth looking at.

## Conflicts



## Problems found

- **Smart App Control**: the release build fails again and again on new build scripts (`icu_normalizer_data`, `num-traits`, `thiserror`…), each new binary being judged on its own. The debug build goes through. It's Pierre's call whether to turn SAC off, which is hard to undo, or to build release elsewhere.
- **The shell hides the TDP in a firmware mode** (silent / performance / turbo): `tdp_watts` becomes `null` because the firmware applies its own limits and we can't read them. It's honest, but confusing (Pierre: "I don't see the TDP anymore"). Suggestion: show "auto (turbo)", or the mode's nominal values once confirmed.
- `canape-ctl` in PowerShell 5.1: you have to escape the `"` (documented).

## FSE boot test

**OK.** `GamingHomeApp = Canape_84zrj1c9kn010!App`, `StartupToGamingHome = 1`, reboot (debug build):

- Boot at 20:27:52. `CanapeService` starts automatically at 20:28:07 (`device = ROG Ally`).
- The FSE activates `canape-shell` **only once**, at 20:28:15, with `windows.gaming:///home`. In M1, with only a message box, it had activated 3 times within 5 s. **[I]** The FSE was retrying because no real window showed up. In any case, the single instance would have absorbed retries.
- Pierre confirms the shell shows up at boot. `canape-ctl` gets the status (66 °C, 100 %) right after.

Winhanced was put back afterwards: `GamingHomeApp = Winhanced_2wdva71107yvr!App`, `StartupToGamingHome = 1`.

## Still open

- Release build and memory measurement under SAC.
- The shell's idle CPU (2 % of one core).
- Displaying the TDP in a firmware mode, and supporting mode 3 (FullSpeed), which Canapé doesn't model.
- `service.log` doesn't trace firmware writes; worth logging every `set_*`.
