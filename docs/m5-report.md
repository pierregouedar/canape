# M5 report (part 1): the shell, the Hardware page and the library on the Ally

Tested on the ROG Ally RC71L on 2026-10-22, from `7585214`, in a Remote Control session that ended before the last items (Pierre had to leave). This report was written afterwards from that session's transcript. **Release** build, installed with `install-dev.ps1 -BuildProfile release`. Winhanced was paused for the tests; it was left paused.

## Results

| Item | Result | Detail |
|---|---|---|
| Release build (shell, service, ctl) | OK | 1 min 44 s |
| Widget, 13-language version (`build-widget.ps1`) | OK | First build of that version, no fix needed. Only a mangled "Canapé" in the script's last line under Windows PowerShell 5.1 (fixed: the scripts are ASCII only) |
| Install | OK | `CanapeService installed and started`, `Registered Canape_84zrj1c9kn010!App`; `device = ROG Ally`, `conflicting programs: []` |
| `cargo test` under Windows | OK after a fix | `steam::tests::scan_fixture` also found Pierre's real Steam games: the fixture pointed to `C:\Program Files (x86)\Steam` (fixed in `f54ded4`) |
| Shell in use (pages, gamepad, power modes, launching a game) | OK, per Pierre | Pierre used it himself (Silent → Performance → Turbo → Silent, Sonic Mania launched twice from the library): "nothing to report". Not checked page by page |
| Accent shared through the service | OK | `m5-check` reads back `#14b8a6`, then restores the original accent |
| Fans | OK | Driven with `canape-ctl` under a heavy WebGL load. Automatic: 2900–3000 / 2700–2800 rpm (CPU / GPU). Silent: down to 0/0 within 15 s. Balanced: 3300/3300. Maximum: 4600–4700. Back to Automatic with `ResetFanCurves -> Ok`. Every `SetFanCurve -> Ok` |
| Controller mode | OK, per Pierre | Keyboard, Mouse, then Gamepad, each `-> Ok`; "OK, Canapé responds" |
| Screen refresh rate | OK, per Pierre | `screen: 60 Hz`, `screen: 120 Hz`; Windows reports `CurrentRefreshRate` 120 |
| Armoury and Command Center buttons | OK, per Pierre | Both act; no `buttons: unknown code` in `shell.log` |
| Library: Steam | OK after a fix | 6 games, with covers (`library_600x900.jpg`, `library_hero.jpg` for Lossless Scaling), but **each listed twice** (fixed after the session, see below) |
| Library: Xbox | OK | Palworld, with its square logo |
| Library: Epic, GOG | Nothing to find | No Epic manifest, no GOG game installed |
| Launching from the library | OK | `library: launching steam:584400` (Sonic Mania) |
| Frame rate limit | **Problem** | See below |
| Auto TDP | **Problem, fixed** | See below; fixed and checked on the Ally in `01d79cf` |

### Memory (`m5-check`, WebGL Aquarium in front)

| Process | Private | Working set |
|---|---|---|
| canape-service | 2.0–2.1 MB | 11.0–11.1 MB |
| canape-shell | 16.7–16.8 MB | 36.6–36.7 MB |
| CanapeWidget | not measured (Game Bar closed) | |

### Bench (`m5-check`, WebGL Aquarium with 30,000 fish)

| Limit | Average FPS | Low FPS | Max temperature |
|---|---|---|---|
| 7 W | 19 | 17 | 47 °C |
| 13 W | 43 | 40 | 53 °C |
| 19 W | 61 | 61 | 64 °C |
| 25 W | 69 | 69 | not recorded |

A first run on the light Aquarium was capped at 120 FPS (the screen) at every step, so it says nothing.

## Problems

### Auto TDP sawing at a low target (fixed)

With a 40 FPS target on the heavy Aquarium, power went 11 ↔ 13 W every 5 to 10 s for 3 minutes and was never learned. One watt above the need already gave 113 % of the target, above the 110 % that released a floor: every raise released its own floor. Floors are now released above 125 % of the target (`FLOOR_RELEASE_RATIO`, test `steep_game_at_a_low_target_settles`, which failed before). On the Ally afterwards: steady at 13 W (≈45 FPS), learned at 13 W within 60 s, a single probe at 11 W in 200 s.

The `m5-check` "learned TDP" check also fails because it waits only 40 s while 30 unchanged ticks are needed after settling: lengthened to 75 s since.

### Steam games listed twice (fixed after the session)

The registry gives `c:/program files (x86)/steam`, `libraryfolders.vdf` gives `C:\\Program Files (x86)\\Steam`: the same folder was scanned twice. Folders are now compared case-insensitively, either slash, without a trailing separator (`folder_key`, with a test). The fix was found on the Ally but not pushed before the session ended; it was redone here. **To check on the Ally.**

### Frame rate limit "None" did not lift the cap (fixed, to check)

The driver starts with `AC 1000 (enabled 0, range 15..=1000)`. From the service, a cap of 40 held Sonic Mania at exactly 40.0 FPS: writing from session 0 works. But "None" wrote 240, and the game stayed near 26 FPS; writing 1000 brought it back to 60 after about 20 s:

```
s1 1000 : 60,0 60,0 60,0 60,0 60,0 58,6 27,9 27,5 27,9 28,4 27,0 27,7
s1 240  : 26,5 25,0 25,0 23,3 26,1 25,4 24,0 25,1 26,2 24,9 25,7 23,5
s1 1000 : 27,8 28,3 27,3 27,2 27,7 25,2 38,2 60,0 60,0 60,0 60,0 60,0
svc 40  : 40,0 40,0 40,0 25,1 27,2 26,8 26,6 24,9
```

The measure is disturbed: Sonic Mania, on its title screen in Silent mode (9–10 W, GPU at 1 %), also dropped to about 27 FPS on its own with 1000. "None" now writes the top of the range the driver reports (1000 on the Ally) instead of 240. The limiter's `enabled` flag, 0 at first, stays at 1 after any write: the ADL call used has no field to clear it. **To measure again on the Ally with a steadier game.**

### Other notes

- At service start the log reads `fan speeds Some(0) / Some(0) rpm`, while the status reads 3200 / 2700 a moment later: the first read probably comes before the EC updates.
- A `LEGOÂ® City Undercover` seen when printing `library.json` from PowerShell 5.1: probably the console reading UTF-8 as ANSI, not checked in the shell.

## Not done (part 2)

Before part 2, install **Unigine Heaven** and **FurMark 2** on the Ally (free, from their official sites; not shipped with Canapé). They give steadier loads than the light Aquarium (capped at 120 FPS) and Sonic Mania's title screen (dropping on its own), see [Choosing a test load](auto-tdp.en.md#choosing-a-test-load): Heaven for the bench, auto TDP and the frame rate limit, FurMark 2 for the fan and temperature curves.


- Installing games without the launchers: Steam (API key in `shell.json`) and Epic (Legendary download, sign-in, install, launch).
- The widget by hand (style, graphs, ±1 W bounds, accent), and its memory.
- Memory at rest and in a real game.
- Per-game refresh rate (applied and restored), starting as the FSE home, language switch, live graphs.
- The frame rate limit again, with the fix above.

State left on the Ally: Winhanced paused, Silent mode, auto TDP off, driver limiter at 1000 (enabled), screen at 120 Hz, fans on Automatic.
