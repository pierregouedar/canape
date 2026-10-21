# Widget Xbox Game Bar (M3)

Réglages rapides en jeu : TDP, mode, auto TDP, FPS, température, batterie. Toute la logique reste dans le service : le widget envoie des requêtes JSON (`canape-core::protocol`) sur `\\.\pipe\canape`.

## Choix

- **C# UWP (.NET Native)** : Game Bar n'accepte que des apps UWP XAML (WinUI 3 non supporté, [XboxGameBarSamples#100](https://github.com/microsoft/XboxGameBarSamples/issues/100)). Projet calqué sur l'exemple officiel `WidgetSampleCS` (MIT).
- **SDK `Microsoft.Gaming.XboxGameBar` 7.2.240903001**, celui des exemples : la 7.3.x actuelle casse la compilation ([#155](https://github.com/microsoft/XboxGameBarSamples/issues/155)). Le bloc `proxyStub` du manifeste correspond à cette version.
- **Pipe ouvert avec `CreateFileFromAppW`**, autorisé depuis un AppContainer à partir de Windows 10 2004 ([#44](https://github.com/microsoft/XboxGameBarSamples/issues/44)). La DACL du pipe accorde lecture/écriture à `ALL APPLICATION PACKAGES` ; à durcir plus tard avec le SID du paquet du widget.
- Une connexion d'abonnement ne fait que recevoir le statut ; chaque commande ouvre sa propre connexion courte (le pipe est synchrone côté Windows, voir `docs/m2-report.md`).

## Compiler et installer sur l'Ally

1. Ajouter le workload UWP aux Build Tools (une fois, plusieurs Go, Terminal administrateur) :

   ```powershell
   $vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath
   & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\setup.exe" modify --installPath $vs `
       --add Microsoft.VisualStudio.Workload.UniversalBuildTools --includeRecommended --quiet --wait
   ```

2. `powershell -ExecutionPolicy Bypass -File packaging\build-widget.ps1` (le certificat de `install-dev.ps1` signe le paquet).
3. Ouvrir la Game Bar (Win+G ou bouton Xbox), menu Widgets, Canapé.
