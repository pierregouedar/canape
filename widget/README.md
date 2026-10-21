# Widget Xbox Game Bar (M3)

Réglages rapides en jeu : TDP, mode, auto TDP, FPS, température, batterie. Toute la logique reste dans le service : le widget envoie des requêtes JSON (`canape-core::protocol`) sur `\\.\pipe\canape`.

## Choix

- **C# UWP (.NET Native)** : Game Bar n'accepte que des apps UWP XAML (WinUI 3 non supporté, [XboxGameBarSamples#100](https://github.com/microsoft/XboxGameBarSamples/issues/100)). Projet calqué sur l'exemple officiel `WidgetSampleCS` (MIT).
- **SDK `Microsoft.Gaming.XboxGameBar` 7.2.240903001**, celui des exemples : la 7.3.x actuelle casse la compilation ([#155](https://github.com/microsoft/XboxGameBarSamples/issues/155)). Le bloc `proxyStub` du manifeste correspond à cette version.
- **Pipe ouvert avec `CreateFileFromAppW`**, autorisé depuis un AppContainer à partir de Windows 10 2004 ([#44](https://github.com/microsoft/XboxGameBarSamples/issues/44)). La DACL du pipe accorde lecture/écriture à `ALL APPLICATION PACKAGES` ; à durcir plus tard avec le SID du paquet du widget.
- Une connexion d'abonnement ne fait que recevoir le statut ; chaque commande ouvre sa propre connexion courte (le pipe est synchrone côté Windows, voir `docs/m2-report.md`).

## Interface

- En-tête : point d'état de la connexion au service, jeu au premier plan (`foreground`, sans `.exe`) et pastille « Profil » quand un profil est appliqué (`active_profile`).
- Tuiles TDP / FPS / température / batterie. La légende de la température devient « Bridé » en jaune quand l'auto TDP retient la puissance à cause de la température (`thermal_limited`) ; celle de la batterie devient « Secteur » en vert sur le chargeur (`on_battery: false`).
- Carte de progression du bench quand le statut contient `bench` (palier, puissance, temps restant). Le bench se lance depuis le shell : aucune commande de bench dans le widget.
- Modes, ±1 W, auto TDP (on/off et cible 30/40/60 FPS). **±1 W n'est actif que si `tdp_watts` est connu** : en mode firmware (silencieux, performance, turbo) le TDP est inconnu et les boutons sont grisés avec la mention « Fixé par le mode du firmware ». `set_auto_tdp` renvoie l'objet `auto_tdp` courant en entier (dont `battery_max_watts` et `max_temp_c`) en ne changeant que `enabled` et `target_fps`.
- Style : panneau sombre (`#16181D`) dont l'opacité suit le réglage de transparence de la Game Bar (`XboxGameBarWidget.RequestedOpacity`), boutons arrondis translucides, sélection en `#2563EB` (états de `ToggleButton` via les ressources « lightweight styling » de `WidgetPage.xaml`). Lisible en 340×360 ; à partir de 420×420 l'espacement et les chiffres grandissent (`AdaptiveTrigger`), jusqu'à 480×480.

## Langues

Toute chaîne visible vient de `CanapeWidget/Strings/<langue>/Resources.resw` : `x:Uid` dans le XAML (clés `Nom.Propriété`, par ex. `ModeSilent.Text`), `ResourceLoader.GetForCurrentView()` dans le code (clés sans point, par ex. `NoGame`), `ms-resource:` dans le manifeste (nom et description du paquet et du widget). Les unités et le format des nombres sont aussi des ressources (`FormatWatts`, `FormatPercent`, `FormatCelsius`) : « 80 % » en français, « 80% » en anglais, « %80 » en turc, « Вт » en russe.

Langues fournies : en-US (langue par défaut, `DefaultLanguage` du projet, repli quand la langue de Windows n'est pas fournie), fr-FR, es-ES, de-DE, it-IT, pt-BR, nl-NL, pl-PL, ru-RU, tr-TR, ja-JP, ko-KR, zh-Hans. Windows choisit d'après la liste des langues préférées de l'utilisateur. Les messages d'erreur renvoyés par le service (`error.message`) restent en anglais : le widget les cite après un préfixe traduit.

### Ajouter une langue

1. Copier `Strings/en-US/Resources.resw` dans `Strings/<balise BCP-47>/Resources.resw` (par ex. `Strings/sv-SE/`) et traduire chaque `<value>`. Garder toutes les clés et les marqueurs `{0}`, `{1}`, `{2}` ; les `<comment>` de la version anglaise décrivent chaque chaîne. Rester court pour les boutons et les légendes de tuiles (environ 10 caractères à 340 px de large).
2. Ajouter `<PRIResource Include="Strings\<balise>\Resources.resw" />` dans `CanapeWidget.csproj`.
3. Rien à changer dans le manifeste : `<Resource Language="x-generate" />` est remplacé à la compilation par toutes les langues trouvées.
4. Recompiler (`packaging\build-widget.ps1`) puis tester : Paramètres Windows > Heure et langue > Langue, mettre la langue en tête, relancer la Game Bar.

Une clé absente d'une langue retombe sur l'anglais ; une clé `ms-resource:` du manifeste absente de l'anglais fait échouer l'empaquetage, une clé `x:Uid` absente laisse le texte vide. Dans le code, `S()` affiche le nom de la clé si elle manque partout.

## Compiler et installer sur l'Ally

1. Ajouter le workload UWP aux Build Tools (une fois, plusieurs Go, Terminal administrateur) :

   ```powershell
   $vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath
   & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\setup.exe" modify --installPath $vs `
       --add Microsoft.VisualStudio.Workload.UniversalBuildTools --includeRecommended --quiet --norestart
   ```

2. `powershell -ExecutionPolicy Bypass -File packaging\build-widget.ps1` (le certificat de `install-dev.ps1` signe le paquet).
3. Ouvrir la Game Bar (Win+G ou bouton Xbox), menu Widgets, Canapé.
