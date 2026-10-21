# M1 report: canape-shell as the FSE home app

Tested on the ROG Ally (Windows 11 10.0.26200, PowerShell 5.1, Rust 1.99.1 MSVC, Windows SDK 10.0.26100), on 2026-10-21.

## What worked

| Step | Result |
|---|---|
| `cargo test` | OK (5 tests) |
| `cargo build --release -p canape-shell` | OK, on the second attempt (see Smart App Control) |
| Embedded manifest | Present in `canape-shell.exe`: `msix publisher="CN=Canape Dev" packageName="Canape" applicationId="App"`, `asInvoker` |
| `packaging\install-dev.ps1` (elevated) | Ran without errors on the first try: certificate created and trusted, `makeappx` + `signtool` OK, `Add-AppxPackage -ExternalLocation` OK |
| Package | `Canape_0.1.0.0_neutral__84zrj1c9kn010`, `SignatureKind = Developer`, `Status = Ok`; AUMID `Canape_84zrj1c9kn010!App` |
| Launch outside the FSE (`explorer.exe shell:AppsFolder\Canape_84zrj1c9kn010!App`) | "Canapé a démarré." box shown, a line in `%LOCALAPPDATA%\Canape\shell.log`; `tasklist /apps` shows `canape-shell.exe (App)` with the package identity |
| `windows.gamingApp` catalog | `AppExtensionCatalog.Open("windows.gamingApp")` lists `Canape` next to `XboxPcApp` and `Winhanced` |
| Settings → Gaming → Full screen experience | Canapé appears in the home app list (checked by Pierre) |
| Real boot test | Pending: see below |

## What I fixed

Nothing in the code or the packaging: the manifests, the SCCD and the script written from Linux worked as-is.

## Problems found

- **Smart App Control is on** on the Ally (`VerifiedAndReputablePolicyState = 1`). On the first release build it blocked the `zmij` build script (os error 4551, CodeIntegrity events 3033/3077). The second try went through. Expect random failures on new unsigned binaries, including `canape-shell.exe` after a rebuild. Turning SAC off is Pierre's call, and it is hard to turn back on.

## FSE state before the test

`HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration`: `GamingHomeApp = Winhanced_2wdva71107yvr!App`, `StartupToGamingHome = 1`. Exported to `$HOME\dev\re\GamingConfiguration.before-canape.reg` (outside the repo).

## Real boot test

Pending. With Pierre's approval: `GamingHomeApp = Canape_84zrj1c9kn010!App`, then reboot. Expected result: a line in `shell.log` dated after the boot (previous boot: 2026-10-20 23:54:48).

## Still open

- Restore Winhanced as the home app after the test.
- Smart App Control vs. dev builds (see above).
