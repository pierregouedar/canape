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
| `windows.gamingApp` catalog | `AppExtensionCatalog.Open("windows.gamingApp")` lists `Canape` next to `XboxPcApp` and another home app |
| Settings → Gaming → Full screen experience | Canapé appears in the home app list (checked by Pierre) |
| Real boot test | **OK**: the FSE activates `canape-shell.exe` at boot (see below) |

## What I fixed

Nothing in the code or the packaging: the manifests, the SCCD and the script written from Linux worked as-is.

## Problems found

- **Smart App Control is on** on the Ally (`VerifiedAndReputablePolicyState = 1`). On the first release build it blocked the `zmij` build script (os error 4551, CodeIntegrity events 3033/3077). The second try went through. Expect random failures on new unsigned binaries, including `canape-shell.exe` after a rebuild. Turning SAC off is Pierre's call, and it is hard to turn back on.

## FSE state before the test

`HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration`: `GamingHomeApp = <PFN of the previous home app>!App`, `StartupToGamingHome = 1`. Exported to a `.reg` file outside the repo before the change.

## Real boot test

With Pierre's approval: `GamingHomeApp = Canape_84zrj1c9kn010!App`, `StartupToGamingHome = 1`, then reboot.

- Boot at 19:35:09. `shell.log` gets three lines, at 19:35:40, 19:35:44 and 19:35:45, all with `args=["windows.gaming:///home"]`. Pierre confirms the box appears at boot.
- **The FSE activates its home app through the URI `windows.gaming:///home`**, passed on the command line. Outside the FSE (AppsFolder), `args=[]`. This is how to tell the two kinds of launch apart.
- Afterwards only one `canape-shell` process is still running (the 19:35:45 one).

The previous home app was put back afterwards: `GamingHomeApp` restored, `StartupToGamingHome` unchanged (1).

## Still open

- **Repeated activations**: three launches within 5 s at boot. Not established: whether the FSE retries because no window shows up quickly enough (a message box may not count as the home window), or because the first two instances were closed. To handle in M2: a single instance (named mutex plus forwarding the activation to the running instance), and a real full-screen window shown right away.
- Smart App Control vs. dev builds (see above).
- The Canapé package stays installed, and the `CN=Canape Dev` certificate stays in `LocalMachine\TrustedPeople`, for the next milestones.
