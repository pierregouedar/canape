# Canapé user guide

[🇫🇷 Version française](user-guide.fr.md) · [📚 Documentation index](README.md)

This guide explains how to install Canapé on a ROG Ally, make it the home app of the full-screen experience (FSE), use it with the gamepad, add the Xbox Game Bar widget, and then troubleshoot and uninstall it.

> [!IMPORTANT]
> Canapé is a personal project under development. **Validated on the Ally**: installation, FSE boot, the service, modes, manual TDP, auto TDP (version `99cad07`) and the first version of the widget. **Written and tested without hardware, but not tried on the Ally yet**: the seven-page shell in 13 languages, the game library, the Hardware page (fans, frame rate limit, controller mode), per-game profiles, the bench, the latest auto TDP optimizations and the new version of the widget. The sections concerned say so.

## Contents

- [Requirements](#requirements)
- [Installing Canapé](#installing-canapé)
  - [1. Turn on Developer Mode](#1-turn-on-developer-mode)
  - [2. Run `install-dev.ps1`](#2-run-install-devps1)
  - [Script options](#script-options)
  - [Using the CI binaries](#using-the-ci-binaries)
  - [Checking the installation](#checking-the-installation)
- [Making Canapé the FSE home app](#making-canapé-the-fse-home-app)
  - [Going back to Winhanced or the Xbox app](#going-back-to-winhanced-or-the-xbox-app)
- [Using Canapé with the gamepad](#using-canapé-with-the-gamepad)
  - [Controls](#controls)
  - [Shared elements](#shared-elements)
  - [Library](#library)
  - [Home](#home)
  - [Performance](#performance)
  - [Hardware](#hardware)
  - [Profiles](#profiles)
  - [Bench](#bench)
  - [Settings](#settings)
- [The Xbox Game Bar widget](#the-xbox-game-bar-widget)
- [Languages](#languages)
- [Troubleshooting](#troubleshooting)
- [Uninstalling](#uninstalling)

## Requirements

| Item | Details |
|---|---|
| Handheld | **ROG Ally RC71L** (the only model tested). The ROG Ally X (RC72LA), ROG Xbox Ally (RC73YA) and ROG Xbox Ally X (RC73XA) are recognized by the code but have never been tried. |
| Windows | Windows 11 **24H2 or later** (the identity package requires version 10.0.26100). |
| Tools | **Visual Studio Build Tools 2022** with the C++ tools and the **Windows SDK** (provides `makeappx` and `signtool`, used even with prebuilt binaries). **Rust** (MSVC toolchain) if you build on the Ally. **Git** to get the repository. |
| Rights | An **elevated** Terminal for the installation (Windows service and certificate). |
| Optional | [GitHub CLI](https://cli.github.com/) (`gh`) to download the CI binaries; the **UWP** workload of the Build Tools for the widget. |

> [!WARNING]

## Installing Canapé

### 1. Turn on Developer Mode

Settings → System → Advanced → **For developers** → Developer Mode: on.

The install script stops with `Developer Mode is off` otherwise (it reads `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock\AllowDevelopmentWithoutDevLicense`).

### 2. Run `install-dev.ps1`

In an **elevated Terminal**, at the root of the cloned repository:

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1
```

The script runs six steps:

| Step | What happens |
|---|---|
| 1/6 | Builds `canape-shell`, `canape-service` and `canape-ctl` (release by default), or takes prebuilt binaries (`-BinariesDir`). |
| 2/6 | Stops and uninstalls the service if it exists, closes the shell, copies the three `.exe` files and `CustomCapability.SCCD` to `C:\Program Files\Canape`. |
| 3/6 | Installs and starts the **`CanapeService`** Windows service (automatic start, LocalSystem account, automatic restart after a crash). |
| 4/6 | On the first run, creates a `CN=Canape Dev` development certificate in `$HOME\dev\canape-cert` and adds it to `LocalMachine\TrustedPeople`. |
| 5/6 | Packs and signs the `Canape.Identity.msix` identity package. |
| 6/6 | Registers the package with `Add-AppxPackage -ExternalLocation`: `canape-shell.exe` gets the `Canape_…!App` identity that the FSE can launch. |

At the end, the script prints the test command, the `canape-ctl watch` command and where the logs are.

> [!CAUTION]
> Never commit the contents of `$HOME\dev\canape-cert` (`.pfx`, `.cer`): it is the key that signs your packages.

Running the script again later updates Canapé: it replaces the binaries and registers the service and the package again. Your settings (`state.json`) are kept.

### Script options

| Option | Default | Purpose |
|---|---|---|
| `-InstallDir <folder>` | `C:\Program Files\Canape` | Where the binaries are installed. |
| `-CertDir <folder>` | `$HOME\dev\canape-cert` | Where the development certificate is created and reused (the widget uses it too). |
| `-BuildProfile release\|debug` | `release` | Cargo profile. `debug` is a fallback when Smart App Control blocks a release build (see [Troubleshooting](#troubleshooting)). |
| `-BinariesDir <folder>` | *(empty)* | Takes `canape-shell.exe`, `canape-service.exe` and `canape-ctl.exe` from this folder instead of building. Handy with the CI binaries. |
| `-SetAsHomeApp` | off | Also writes `GamingHomeApp` in the registry to pick Canapé as the FSE home app (see below). |

### Using the CI binaries

GitHub Actions builds the release binaries on Windows at every push and publishes them as the `canape-windows-x64` artifact. No Rust needed on the Ally, and no building under Smart App Control:

```powershell
gh run download --repo pierregouedar/canape --name canape-windows-x64 --dir $HOME\dev\canape-bin
powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1 -BinariesDir $HOME\dev\canape-bin
```

You still need the Windows SDK (packing and signing the identity package) and the cloned repository (the script reads `packaging\identity`).

### Checking the installation

1. **The service**: `Get-Service CanapeService` should show `Running`.
2. **Telemetry**: `& "C:\Program Files\Canape\canape-ctl.exe" watch` prints one JSON status per second (Ctrl+C to quit).
3. **The shell outside the FSE**: run the command the script printed, for example:
   ```powershell
   explorer.exe shell:AppsFolder\Canape_84zrj1c9kn010!App
   ```
   The part after `Canape_` depends on your certificate; the script prints the right value (`Registered …`).

## Making Canapé the FSE home app

1. Open **Settings → Gaming → Full screen experience**.
2. Pick **Canapé** as the home app.
3. Turn on starting in the full-screen experience if you want the handheld to boot straight into Canapé.
4. Reboot: Windows launches `canape-shell.exe` with the `windows.gaming:///home` argument.

If Canapé is not in the list, run the script again with `-SetAsHomeApp`. It writes the undocumented value `HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration\GamingHomeApp = <PFN>!App` that the FSE reads to know which app to launch.

> [!NOTE]
> Booting into the FSE was validated on the Ally in M1 and M2 (a single activation at boot, service already up). It has not been retested since, as nothing changed on the startup side.

### Going back to Winhanced or the Xbox app

Just pick the other app in **Settings → Gaming → Full screen experience**. Canapé stays installed and can be picked again at any time.

To do it from the command line, put back the previous `GamingHomeApp` value. For example, for Winhanced:

```powershell
Set-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration" -Name GamingHomeApp -Value "Winhanced_2wdva71107yvr!App"
```


## Using Canapé with the gamepad

> [!NOTE]
> The seven-page shell described here has not been tried on the Ally yet: only its previous (single-page) version and gamepad navigation were validated. The game library in particular was only tested on Linux, with a fake library: finding the games, their covers and launching them on Windows are still to be checked. The same goes for the Hardware page, whose writes to the fans, the graphics driver and the controller have never reached a real Ally (see [Hardware](#hardware)). Screenshots are rendered on Linux with the hardware simulator and made-up values; a few texts may differ slightly from the current version.

### Controls

| Button | Action |
|---|---|
| **LB / RB** | Previous / next page (Library → Home → Performance → Hardware → Profiles → Bench → Settings, wrapping around) |
| **D-pad** or **left stick** | Move to the next control (down, right) or the previous one (up, left). In the library, moves in the grid of games in all four directions. Held, the move repeats after 0.4 s. |
| **A** | Select the focused control; in the library, launch the game |
| **B** | Back to the library |

The gamepad is only read while Canapé is in the foreground: a game launched from Canapé keeps the whole gamepad. The focused control has a white outline. On a keyboard (development), the same actions are Tab / Shift+Tab (the arrow keys in the library), Enter, Esc and Page Up / Page Down.

### Shared elements

- **Top**: the tabs (the current one is underlined in the accent colour), the battery (with a bolt when plugged in) and the word "Offline" when the service does not answer.
- **Bottom**: gamepad hints and, on the right, the outcome of the last command ("Profile saved", "Service unreachable", "Service error: …"). Error messages from the service stay in English.
- An unknown value shows as "—" (for example the frame rate when no game is running).

### Library

<img src="images/en/library.png" alt="Library page" width="720">

<sub>A made-up library (`tools/fake-library.py`): titles and covers are generated.</sub>

The first page, the one Canapé opens on: the games installed on the console, as covers, the most recently played first.

| Area | Contents |
|---|---|
| **Top left** | The selected game: its title, its store (as a word: STEAM, EPIC, GOG, XBOX) and when it was last played ("Played today", "Played yesterday", "Played in the last 7 days"…). "Launching…" after A. |
| **Filters** | All, Steam, Epic, GOG, Xbox, then the order: Recent (last played first, then by name) or A–Z. |
| **Grid** | Six covers per row. The selected one is larger, with a white outline. Under All, each cover shows its store at the bottom left. A game without art shows its title on a card tinted with the accent colour. |

**Moving around**: the D-pad (or the stick) moves in the grid; down from a row above a shorter last row lands on the last game. Up from the first row reaches the filters: left and right pick one, **A** applies it, down goes back to the games. **A** on a game launches it; Canapé shows "Launching…" for a few seconds, then puts the game first in the list. The page scrolls by itself to keep the selected row at the top.

**Where the games come from**

| Store | What Canapé reads | Launch | Covers |
|---|---|---|---|
| **Steam** | The library folders (`steamapps\libraryfolders.vdf`) and each game's manifest (`appmanifest_*.acf`). Tools (Steamworks redistributables, Proton, SteamVR) are left out. | Through Steam (`steam://rungameid/…`) | Steam's own cache: the art you picked in Steam, else the 600×900 portrait, else the hero or header image (shown whole, on a blurred backdrop) |
| **Epic Games** | The launcher's manifests (`%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`). DLC, engines and unfinished installs are left out. | Through the Epic launcher | Downloaded (see below), else a title card |
| **GOG** | The registry (`HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`), where GOG Galaxy and the offline installers register each game. DLC are left out. | The game's executable directly, without Galaxy | Downloaded (see below), else a title card |
| **Xbox app** | The `XboxGames` folders of every drive (`Content\MicrosoftGame.config`). | Through Windows (`shell:AppsFolder\…`) | The game's square logo |

- The last played time comes from Steam for Steam games, and from Canapé for every game launched from it.
- A game installed from two stores shows once: the copy played last, else the Steam one, then GOG, Epic and Xbox.
- The list shows at once from the last scan; a new scan runs in the background at every start, and again when you come back to the library after a minute away. A game installed meanwhile appears then.
- Xbox app games installed outside an `XboxGames` folder are not found.
- Canapé launches games itself, without rights: like every app of the session.

**Downloaded covers**

A game with no art on the device can get one from the internet after each scan (**Settings → Download missing covers**, on by default):

- **Steam games**: the 600×900 portrait from Steam's public store servers, no account needed. Useful for a game whose art Steam has not cached yet.
- **Every store** (Epic, GOG, Xbox included), with a free **SteamGridDB** API key: create an account on [steamgriddb.com](https://www.steamgriddb.com), copy the key from *Preferences → API*, and put it in `%LOCALAPPDATA%\Canape\shell.json` as `"steamgriddb_key": "…"`. Steam games are looked up by their Steam id, the others by title; only static portrait images are taken (no animated, NSFW or humour grids). The key is only ever sent to SteamGridDB's API.

Covers are saved in `%LOCALAPPDATA%\Canape\covers` and reused; a game with no art anywhere is asked for again after a week. At most 60 downloads per scan. Turning the setting off keeps the covers already downloaded but fetches no more.

> [!NOTE]
> Not tried on the Ally yet: the download goes through WinHTTP (built into Windows) and was only tested on Linux against recorded answers.

### Home

<img src="images/en/home.png" alt="Home page" width="720">

| Area | Contents |
|---|---|
| **Power draw and power limit** | On the left, what the APU actually draws; on the right, the limit applied ("Auto" when a firmware mode picks its own limits, or "—"). Below: "Auto TDP, target 60 FPS", "Set by the firmware: Turbo mode" or "Manual TDP". |
| **Power graph** | The last minute, one point per second: the power draw (filled curve, in the accent colour) under the limit (light stepped line). The scale adapts in 5 W steps. A gap in the curve marks a lost connection to the service. |
| **States** | "Thermal limit reached" (yellow) when the temperature limit is holding power down, "Bench running". |
| **GPU and frame rate** | GPU activity with its graph of the last minute (0 to 100 %), and the game's frame rate. |
| **Tiles** | CPU temperature (yellow from 80 °C, red from 90 °C), battery ("On battery" or "Plugged in"). |
| **Now playing** | The foreground program Canapé detected, and the profile in use, if any. |
| **Shortcuts** | Auto TDP (on / off), Silent, Performance, Turbo. |
| **Yellow warning** | Shows at the top when another program also drives the hardware. |

### Performance

<img src="images/en/performance.png" alt="Performance page" width="720">

<sub>The same page in Russian: [ru/performance.png](images/ru/performance.png).</sub>

**Power mode**

| Tile | Effect |
|---|---|
| Silent | ASUS silent firmware mode. The firmware sets its own limits, which Canapé cannot read: the TDP shows as "Auto". |
| Performance | Balanced firmware mode. |
| Turbo | Turbo firmware mode. |
| Manual | Manual limit: keeps the current power if known, otherwise starts from **15 W**. |

Picking a firmware mode turns auto TDP off.

**Manual TDP**: − / + change the limit by 1 W, within the range of the model the service detected (see below). The buttons are disabled in a firmware mode ("Choose Manual to set the power yourself."). Changing the TDP by hand turns auto TDP off.

**Auto TDP**

| Control | Effect |
|---|---|
| On / Off | Turns auto TDP on or off. |
| Target frame rate | 30, 40, 45, 60, 90 or 120. Picking a target **also turns** auto TDP on. |
| Minimum / maximum power | − / + in 1 W steps. The minimum cannot pass the maximum. Both stay within the range of the model the service detected and reports: 5 to 30 W on the ROG Ally and Ally X, 5 to 35 W on the ROG Xbox Ally models. Without that information (older service), the shell falls back to 5 to 35 W and the service still brings the values into the model's range. |
| Maximum on battery | None, 10, 15 or 20 W: a ceiling applied only on battery. |
| Temperature limit | None, 80, 85 or 90 °C: above it, power only goes down. |

On the right, the same graph as on Home shows the power draw against the limit live: you can watch auto TDP bring the power down or up.

How it works in detail: [Auto TDP and bench](auto-tdp.en.md).

> [!TIP]
> While a game profile is active, these settings change **the profile**, not the global settings.

### Hardware

<img src="images/en/hardware.png" alt="Hardware page" width="720">

> [!NOTE]

**Fans**

| Tile | Curve (CPU and GPU fans alike) |
|---|---|
| Automatic | The firmware's own curve for the current power mode. Canapé removes its curves. |
| Silent | Fans stopped up to 45 °C, then a slow rise: 26 % at 75 °C, 45 % from 95 °C. |
| Balanced | From 10 % at 40 °C to 75 % from 92 °C, close to ASUS's balanced behaviour. |
| Maximum | 35 % from 30 °C, full speed from 80 °C: loud, but the coolest. |

- Below the tiles: the CPU fan and GPU fan speeds, in rpm, and the CPU temperature. "—" when the firmware gives no reading.
- On the right: the CPU fan's curve, from 20 to 110 °C across and from 0 to 100 % up. With Automatic, the panel only says "The power mode's own curve".
- A preset **survives power mode changes**: the firmware puts its own curves back at every mode change, and the service writes the preset again right after.
- **Automatic** writes the current power mode again: that is how the firmware takes its curves back. In Manual, the service then puts your TDP back.
- **Safety**: every curve is checked before it reaches the firmware. The fan may never slow down as the temperature rises, and it must run at least at 25 % from 80 °C, 30 % from 90 °C and 40 % from 100 °C, just under ASUS's own Silent mode. A curve that would leave the fans stopped when hot is refused. The APU also throttles itself at its temperature limit, but a quiet curve makes the console hot to hold.
- **Custom**: when the curves in use match no preset (set with `canape-ctl`, or a different preset on each fan), no tile is selected and the page shows "Custom curves set by another client."

**Frame rate limit**

None, 30, 40, 45, 60, 90 or 120. The AMD graphics driver holds every game at or below that rate, on AC and on battery: less heat, less noise, longer battery life. **None** removes the limit.

> [!TIP]
> With auto TDP, set its target **at or below** the limit. Auto TDP regulates power to reach its target: with a limit of 40 and a target of 60, the target can never be reached and auto TDP would raise power to its maximum for nothing. A limit equal to the target is ideal: the game sits at its cap and auto TDP takes back the watts it does not need (see [Auto TDP and bench](auto-tdp.en.md#the-frame-cap-and-the-idle-gpu-rule)).

**This game**

The foreground program ("No game in the foreground" otherwise) and the **Keep fans and limit for this game** button. It saves the current fan curves and frame rate limit in that game's profile:

- The other settings of the profile (auto TDP, learned watts) are kept. Without a profile, one is created with the current auto TDP settings.
- **Automatic** is saved too: that game will then run on the firmware's curves even if a preset is in use the rest of the time.
- **None** is saved as "follow the global limit": a profile cannot force "no limit" while a global limit is set.
- When the game comes to the foreground, its curves and limit apply; when it leaves, the global ones come back.
- While the game is in front, picking a preset or a limit changes **its profile** if the profile has its own fans (or its own limit); otherwise the change is global and stays after the game. A profile made from the Profiles page or the bench has neither: it follows the global fans and limit.

**Controller**

| Tile | Mode of the built-in controller |
|---|---|
| Gamepad | Xbox gamepad (XInput), for games. |
| Keyboard | The sticks and buttons send keys, for games that only know the keyboard. |
| Mouse | Desktop mode: the right stick moves the pointer. |

The mode is global (not per game), remembered and re-applied when the service starts. Canapé does not read it back from the controller: no tile is selected until Canapé has set it once. Only the ROG Ally and Ally X controllers are known; on the ROG Xbox Ally models the request returns an error.

> [!WARNING]
> Canapé reads the gamepad through XInput. In Keyboard or Mouse mode the controller should no longer show up as an Xbox gamepad, so Canapé may stop answering it: come back to **Gamepad** with the touch screen, or with `canape-ctl` (see [Troubleshooting](#canape-ctl-talking-to-the-service)).

### Profiles

<img src="images/en/profiles.png" alt="Profiles page" width="720">

A profile ties an executable (for example `eldenring.exe`) to auto TDP settings and, if you saved them from the [Hardware](#hardware) page, to fan curves and a frame rate limit. When that program comes to the foreground, the service applies the profile; when it leaves, the global settings come back.

- **Current game** (left): the foreground program, the settings that will be saved (target, power, maximum on battery, temperature limit) and the **Save as profile** (or **Update the profile**) button. The button is disabled until a game is detected.
  - If auto TDP is on, the current power is also saved as **learned watts**: the next session of that game starts right there.
  - The fan curves and frame rate limit already in the profile are kept. This page does not show them.
- **Profiles** (right): the list sorted by name, with the target, the power range and the learned watts. The profile in use is highlighted ("In use"). **Delete** asks for confirmation: a first press of A shows "Confirm", a second one deletes. Moving away cancels.

> [!NOTE]
> **How Canapé detects the game**: every 2 seconds the shell checks which program is in the foreground and tells the service (which, in session 0, cannot see windows). Explorer, Game Bar, the Start menu, Task Manager and Canapé itself are ignored: going back to Canapé does not "leave" the game while it is still running. Another program counts as a game when it **fills the screen** (full screen or borderless window) or **already has a profile**: a browser or Discord in an ordinary window does not replace the game running behind it. To create a profile for a windowed game, switch it to full screen while you save the profile. The shell must be running for profiles to work.

Profiles and learned watts are covered in [Auto TDP and bench](auto-tdp.en.md#per-game-profiles).

### Bench

<img src="images/en/bench-results.png" alt="Bench page with results" width="720">

The bench plays the foreground game at several power limits, from 25 W down to 7 W, measures each one and suggests the right value.

1. Start the game and leave a **demanding, steady scene** running (not a menu).
2. Go back to Canapé (Xbox button or Alt+Tab), **Bench** page.
3. Pick the **step** (2, 3 or 5 W) and the measuring time **per step** (10, 15 or 30 s). The total time shows under **Start** (about 2:20 with 3 W and 15 s).
4. **Start**, then go back to the game. The limit changes on its own; each step waits 5 s before measuring.

<img src="images/en/bench-running.png" alt="Bench running" width="720">

During a bench, the mode, the TDP and auto TDP are locked ("a bench is running: cancel it first"); **Cancel** stops the bench and keeps the steps already measured. At the end, or on cancel, the service restores the previous state.

**Results**: for each step, the power, the average FPS (with a relative bar), the lowest FPS (the worst second), FPS per watt and the maximum temperature. The sweet spot is highlighted. Three recommendations follow:

| Card | Meaning |
|---|---|
| **Sweet spot** | The lowest power within 5 % of the best result: past it, more watts buy almost nothing. |
| **For N FPS** | The lowest power that holds the current auto TDP target, on average. "Target not reached in this run" otherwise. |
| **Most efficient** | The most frames per watt: the most battery-friendly setting. |

Each card offers **Set as TDP** (manual limit at that value, auto TDP off) and **Save for the game** (creates or replaces the game's profile: auto TDP on, target and ceiling taken from the measurement). The exact computation is in [Auto TDP and bench](auto-tdp.en.md#the-bench).

> [!NOTE]
> The bench has not been tried on the Ally yet.

### Settings

<img src="images/en/settings.png" alt="Settings page" width="720">

- **Language**: "Automatic" (the Windows language) or one of the 13 languages. The change is immediate and remembered. See [Languages](#languages).
- **Accent colour**: three sliders, **Hue**, **Saturation** and **Brightness**, a preview with the `#RRGGBB` code and 8 **presets**. With the gamepad, A on a slider grabs it, the D-pad or the stick moves it, A again lets go. The whole UI follows live; the colour is saved in `shell.json` and sent to the service once you stop adjusting, and **the widget follows the accent colour**.
- **Canapé**: version, service connection state, and the **Download missing covers** switch (see [Library](#library)): the line under it says where covers come from (Steam only, or Steam and SteamGridDB when a key is set).

| Teal accent | Amber accent |
|---|---|
| <img src="images/accent-teal.png" alt="Home with a teal accent" width="400"> | <img src="images/accent-amber.png" alt="Home with an amber accent" width="400"> |

<sub>The same settings in German: [de/settings.png](images/de/settings.png).</sub>

## The Xbox Game Bar widget

The widget gives you the quick settings without leaving the game. All the logic stays in the service: the widget is just a remote control.

**Install** (once, then at every widget update):

1. Add the UWP workload to the Build Tools (elevated Terminal, several GB):
   ```powershell
   $vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath
   & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\setup.exe" modify --installPath $vs `
       --add Microsoft.VisualStudio.Workload.UniversalBuildTools --includeRecommended --quiet --norestart
   ```
2. Build, sign and install (no elevation needed; the `install-dev.ps1` certificate must exist):
   ```powershell
   powershell -ExecutionPolicy Bypass -File packaging\build-widget.ps1
   ```

**Open**: Game Bar (Win+G or the Xbox button) → **Widgets** menu → **Canapé**. You can pin it to keep it over the game.

**Contents**

| Area | Details |
|---|---|
| Header | Connection state as text ("Connecting…", "Service not connected"), foreground game (without `.exe`), "Profile" in the accent colour when a profile is applied. |
| Tiles | TDP, FPS, temperature (caption "Throttled" in yellow when the temperature limit is holding power down), battery (caption "Plugged in" in green on the charger). |
| Bench | Step, power and time left while a bench runs (started from the shell). |
| Modes | Silent, Performance, Turbo. |
| ±1 W | Only active when the TDP is known; disabled in a firmware mode ("Set by the firmware mode"). Stays within the model's range reported by the service. |
| Auto TDP | On / off and target 30, 40 or 60 FPS. The other settings (minimum, maximum, battery, temperature) are kept as they are. |

The widget has no Manual mode, no bench button, no profile management and no fan, frame rate limit or controller settings: those stay in the shell. Its panel follows Game Bar's transparency setting, and the widget follows the accent colour picked in the shell.

> [!NOTE]
> The **first version** of the widget was validated on the Ally (M3) through Win+G on the desktop. The **current version** (13 languages, new look, ±1 W disabled in firmware modes and bounded by the model's range) has not been built yet. Opening it from the Xbox button in the FSE, with Canapé as the home app, has not been tested yet.

## Languages

Canapé is translated into 13 languages:

| Language | Shell | Widget |
|---|---|---|
| English | `en` (source language) | `en-US` (default fallback) |
| Français | `fr` | `fr-FR` |
| Español | `es` | `es-ES` |
| Deutsch | `de` | `de-DE` |
| Italiano | `it` | `it-IT` |
| Português | `pt` | `pt-BR` |
| Nederlands | `nl` | `nl-NL` |
| Polski | `pl` | `pl-PL` |
| Русский | `ru` | `ru-RU` |
| Türkçe | `tr` | `tr-TR` |
| 日本語 | `ja` | `ja-JP` |
| 한국어 | `ko` | `ko-KR` |
| 简体中文 | `zh_CN` | `zh-Hans` |

<img src="images/ja/home.png" alt="Home in Japanese" width="480">

- **Shell, Automatic mode** (default): the first language in Windows' preferred languages list that is bundled, otherwise English. Any Chinese variant shows Simplified Chinese.
- **Shell, manual choice**: **Settings → Language** page. The choice is saved in `%LOCALAPPDATA%\Canape\shell.json` and applied at once.
- **Widget**: it always follows Windows' preferred languages (Settings → Time & language → Language); restart Game Bar after a change.
- Error messages returned by the service stay in English, after a translated prefix.


These programs write the same power limits to the firmware, and some also the fan curves, the driver's frame rate limit or the controller mode: if two of them run, the last one to write wins, and Canapé's auto TDP or fan preset can be overridden without notice.

Every 5 seconds, the service looks for these processes and shows a yellow warning in the shell (and in the widget):

| Program | Process detected |
|---|---|
| Armoury Crate | `ArmouryCrate.Service.exe` |

What to do:

- **Armoury Crate**: stop its services (`services.msc`) or turn it off.
  ```powershell
  ```

Other ASUS services (ASUS Optimization, System Analysis, ROG Live Service) and RTSS are not reported: as far as we know, they do not write the power limits.

## Troubleshooting

### Files and logs

| File | Contents |
|---|---|
| `%ProgramData%\Canape\service.log` | Service log: detected model, ADL state at startup, every command that changes the state and its response (`SetTdp { watts: 12 } -> Ok`), conflicting programs, errors. |
| `%ProgramData%\Canape\state.json` | Last applied state, re-applied when the service starts: mode, TDP, auto TDP, fan curves, frame rate limit, charge limit, controller mode, profiles, last bench. |
| `%LOCALAPPDATA%\Canape\shell.log` | Shell log: launches (with their arguments), chosen language, ADL state, service errors. |
| `%LOCALAPPDATA%\Canape\shell.json` | Shell preferences: `language`, `accent`, `online_covers`, `steamgriddb_key`. |
| `%LOCALAPPDATA%\Canape\covers\` | Downloaded covers (`<store>_<id>.jpg` / `.png`, `.miss` markers). |
| `%LOCALAPPDATA%\Canape\library.json` | The games of the last scan, shown at once on the next start. Can be deleted: the next scan writes it again. |
| `%LOCALAPPDATA%\Canape\recent.json` | When each game was last launched from Canapé (for the "Recent" order). |

Every line starts with the time in Unix seconds.

### `canape-ctl`: talking to the service

`canape-ctl` sends one JSON request to the service and prints the response; `watch` prints the status every second.

```powershell
$ctl = "C:\Program Files\Canape\canape-ctl.exe"
& $ctl watch
```

> [!WARNING]
> **Windows PowerShell 5.1** (the one on the Ally) drops the double quotes of an argument passed to an `.exe`. Escape them with `\"`:
> ```powershell
> & $ctl '{\"type\":\"set_tdp\",\"watts\":12}'
> ```

Examples (written in readable form here; escape the quotes in PowerShell 5.1):

| Request | Effect |
|---|---|
| `{"type":"get_status"}` | Full status, once |
| `{"type":"set_performance_mode","mode":"turbo"}` | Mode `silent`, `performance`, `turbo` or `manual` |
| `{"type":"set_tdp","watts":12}` | Manual TDP (turns auto TDP off) |
| `{"type":"set_auto_tdp","settings":{"enabled":true,"target_fps":60,"min_watts":7,"max_watts":25}}` | Auto TDP |
| `{"type":"set_battery_charge_limit","percent":80}` | Battery charge limit (40 to 100 %) |
| `{"type":"reset_fan_curves"}` | Fans back to the firmware's curves (Automatic) |
| `{"type":"set_fps_limit","fps":null}` | Removes the frame rate limit (or `"fps":40` for a limit, 30 to 120) |
| `{"type":"set_controller_mode","mode":"gamepad"}` | Controller back to gamepad mode (`wasd`, `mouse`) |
| `{"type":"list_profiles"}` | Saved profiles |
| `{"type":"get_bench_report"}` | Last bench |
| `{"type":"cancel_bench"}` | Cancels the running bench |

The exit code is 0 when the service answered without an error, 1 otherwise. The full list of requests is in the [developer guide](developer-guide.en.md#ipc-protocol).

### Common problems

| Symptom | Likely cause and fix |
|---|---|
| "Service not connected" at the bottom of the shell, "Offline" at the top | The service is not running: `Get-Service CanapeService`, then `Start-Service CanapeService` (elevated). Check `service.log`. |
| The widget shows "Service not connected" and "The Canapé service isn't running." | Same cause: the `\\.\pipe\canape` pipe does not exist. |
| FPS stay at "—" in game | The AMD driver only counts frames once armed from the user session: the shell does it at startup (line `adl: frame metrics started` in `shell.log`). Launch Canapé at least once after boot. |
| The TDP shows "Auto" and −/+ are disabled | A firmware mode (Silent, Performance, Turbo) is active: the firmware does not report its limits. Pick **Manual**. |
| Auto TDP does not move | It only acts while a game runs: at least 5 FPS **and** at least 15 % GPU activity. On the desktop or in a still menu, it keeps the current value. |
| The power changes on its own | Another tool drives the hardware (yellow warning), or a game profile just kicked in ("Profile in use" on Home). |
| The fans or the frame rate limit change on their own | A game profile with its own fans or limit just kicked in or ended, or another tool rewrote them (yellow warning). |
| "Service error: GPU driver refused: ADL2_FPS_Settings_Set… (session 0)" on the frame rate limit | The AMD driver refuses the limit from the service. This case has not been tried yet: note the message (it is also in `service.log`) and report it. |
| The gamepad no longer moves around Canapé after picking Keyboard or Mouse | Expected: the controller no longer acts as an Xbox gamepad. Tap **Gamepad** on the touch screen, or send `{"type":"set_controller_mode","mode":"gamepad"}` with `canape-ctl`. |
| Canapé is not offered in Settings → Gaming | Run `install-dev.ps1 -SetAsHomeApp` again. Check that the package is installed: `Get-AppxPackage -Name Canape`. |
| `Developer Mode is off` | Turn on Developer Mode (see [above](#1-turn-on-developer-mode)). |
| `makeappx.exe not found` | Install the Windows SDK with the Build Tools. |
| `cargo build` fails with `os error 4551` | **Smart App Control** is blocking an unsigned build script. Retry (it sometimes passes), use `-BuildProfile debug`, or install the [CI binaries](#using-the-ci-binaries). Turning Smart App Control off (Windows Security → App & browser control) solves it but is hard to turn back on. |
| `canape-ctl` answers `invalid message` | Quotes dropped by PowerShell 5.1: escape them (see above). |

To start over with the settings: stop the service, delete `%ProgramData%\Canape\state.json`, start the service again. Profiles and the last bench are lost then.

## Uninstalling

There is no uninstall script yet. First, on the **Hardware** page, put the fans back on **Automatic**, the frame rate limit on **None** and the controller on **Gamepad**: once the service is gone, nothing will undo them. Then, in an elevated Terminal:

1. **Pick another home app** in Settings → Gaming → Full screen experience (see [Going back to Winhanced or the Xbox app](#going-back-to-winhanced-or-the-xbox-app)).
2. **Remove the service**:
   ```powershell
   & "C:\Program Files\Canape\canape-service.exe" uninstall
   ```
3. **Remove the packages** (shell identity and widget):
   ```powershell
   Get-AppxPackage -Name Canape | Remove-AppxPackage
   Get-AppxPackage -Name Canape.Widget | Remove-AppxPackage
   ```
4. **Delete the files**:
   ```powershell
   Get-Process canape-shell -ErrorAction SilentlyContinue | Stop-Process -Force
   Remove-Item -Recurse "C:\Program Files\Canape", "$env:ProgramData\Canape", "$env:LOCALAPPDATA\Canape"
   ```
5. **Remove the development certificate** (if nothing else uses it):
   ```powershell
   Get-ChildItem Cert:\LocalMachine\TrustedPeople, Cert:\CurrentUser\My |
       Where-Object Subject -eq "CN=Canape Dev" | Remove-Item
   Remove-Item -Recurse "$HOME\dev\canape-cert"
   ```

