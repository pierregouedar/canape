# Widget Xbox Game Bar (M3)

Réglages rapides en jeu : TDP, mode, auto TDP, FPS, température, batterie. Toute la logique reste dans le service : le widget envoie des requêtes JSON (`canape-core::protocol`) sur `\\.\pipe\canape`.

## Choix

- **C# UWP (.NET Native)** : Game Bar n'accepte que des apps UWP XAML (WinUI 3 non supporté, [XboxGameBarSamples#100](https://github.com/microsoft/XboxGameBarSamples/issues/100)). Projet calqué sur l'exemple officiel `WidgetSampleCS` (MIT).
- **SDK `Microsoft.Gaming.XboxGameBar` 7.2.240903001**, celui des exemples : la 7.3.x actuelle casse la compilation ([#155](https://github.com/microsoft/XboxGameBarSamples/issues/155)). Le bloc `proxyStub` du manifeste correspond à cette version.
- **Pipe ouvert avec `CreateFileFromAppW`**, autorisé depuis un AppContainer à partir de Windows 10 2004 ([#44](https://github.com/microsoft/XboxGameBarSamples/issues/44)). La DACL du pipe accorde lecture/écriture à `ALL APPLICATION PACKAGES` ; à durcir plus tard avec le SID du paquet du widget.
- Une connexion d'abonnement ne fait que recevoir le statut ; chaque commande ouvre sa propre connexion courte (le pipe est synchrone côté Windows, voir `docs/m2-report.md`).

## Interface

Même langage visuel que la page Performance du shell : panneau sombre, chiffres tabulaires, couleur d'accent réservée aux valeurs et au graphe.

- En-tête : jeu au premier plan (`foreground`, sans `.exe`) ou l'état de la connexion (« Connexion… », « Service non connecté ») ; à droite, « profil du jeu » quand son profil s'applique (`active_profile`), ou « Profil : <nom> » quand c'est celui d'un autre programme.
- Trois chiffres : **consommation** (`power_watts`, une décimale), **images par seconde** (`fps`) et **température CPU** (`cpu_temp_c` ; la légende devient « Bridé » en ambre quand `thermal_limited`).
- Graphe de la dernière minute (60 points, un par statut poussé chaque seconde) : la consommation en couleur d'accent, la limite (`tdp_watts`) en pointillés gris. Une valeur `null` coupe la courbe. `LiveGraph.cs` reconstruit deux petits `Path` par statut, rien entre deux statuts, et ne redessine pas quand la Game Bar masque le widget. L'historique est vidé à la déconnexion.
- Carte de progression du banc d'essai quand le statut contient `bench` (palier, puissance, temps restant). Le banc d'essai se lance depuis le shell.
- **Onglets** Puissance et Matériel : une pastille glisse sous l'onglet choisi (260 ms). À l'ouverture du widget et à chaque changement d'onglet, `WidgetPage.Reveal` fait entrer les lignes une à une de gauche à droite (−14 px, fondu, 460 ms, 70 ms d'écart, comme `RevealLine` dans le shell) ; une valeur ‹ › qui change glisse de 6 px (`Bump`).
- Onglet Puissance, **mode** : quatre tuiles avec icône, Silencieux, Performance, Turbo et Auto. La tuile choisie est blanche, texte et icône sombres. Auto envoie `set_auto_tdp` (activé) ; un mode firmware envoie `set_performance_mode`, qui coupe l'auto TDP côté service.
- **Ligne ‹ valeur ›** : en auto TDP, « Objectif » passe d'un cran à l'autre (30, 40, 45, 60, 90, 120 FPS, comme le shell) ; sinon « Limite de puissance » ±1 W, **actif seulement quand `tdp_watts` est connu** (en mode firmware, la mention « Fixé par le mode du firmware » s'affiche et les chevrons sont grisés), borné par la plage du modèle (`tdp_min_watts`, `tdp_max_watts`). `set_auto_tdp` renvoie l'objet `auto_tdp` courant en entier (dont `battery_max_watts` et `max_temp_c`) en ne changeant que `enabled` et `target_fps`.
- **Puissance** : une barre à l'échelle du modèle (5 à 35 W si le service ne la donne pas) avec la plage de l'auto TDP (deux poignées ; le plafond sur batterie remplace le maximum sur batterie) ou la limite manuelle (une poignée), et un trait sur la consommation du moment avec sa valeur. Lecture seule : la plage se règle dans le shell.
- **Enregistrer pour ce jeu**, en bas de chaque onglet, actif avec un jeu au premier plan : `save_foreground_profile` avec `part` `power` ou `hardware` ; le service garde le reste du profil. Le bouton affiche « Profil enregistré » 2 s.
- Onglet Matériel : **ventilateurs** Auto / Équilibré / Maximum (`set_fan_preset` avec `null`, `balanced` ou `max_cooling` ; la tuile vient de `fan_preset` et de `fan_curves`, « Courbes personnalisées » quand d'autres courbes sont en place), **limite de FPS** ‹ › (Aucune, 30, 40, 45, 60, 90, 120 ; `set_fps_limit`), **manette** Manette / Clavier / Souris (`set_controller_mode`).
- Pied : activité GPU, glyphe Segoe MDL2 de la batterie (niveau, ou prise en charge) et pourcentage, et **Ouvrir Canapé** (`Launcher.LaunchUriAsync("canape://home")`, protocole déclaré par le paquet d'identité du shell : la seconde activation passe au shell déjà lancé, qui revient devant).
- Avertissement en ambre quand un autre programme pilote le matériel, erreur de la dernière commande en rouge clair.
- Panneau `#121317` dont l'opacité suit le réglage de transparence de la Game Bar (`XboxGameBarWidget.RequestedOpacity`).
- Accent : `Status.accent` (`#rrggbb`, choisi dans le shell et poussé par le service), `#FF3D5E` par défaut comme le shell. `WidgetPage.ApplyAccent` recolore sur place les pinceaux d'accent ; `Palette.cs` en tire une variante lisible en texte sur le panneau (4,5:1).
- Taille : 400 × 600 par défaut, de 340 × 460 à 480 × 760 (défile si la fenêtre est plus petite que le contenu).

## Langues

Toute chaîne visible vient de `CanapeWidget/Strings/<langue>/Resources.resw` : `x:Uid` dans le XAML (clés `Nom.Propriété`, par ex. `ModeSilent.Text`), `ResourceLoader.GetForCurrentView()` dans le code (clés sans point, par ex. `NoGame`), `ms-resource:` dans le manifeste (nom et description du paquet et du widget). Les unités et le format des nombres sont aussi des ressources (`FormatWatts`, `FormatPercent`, `FormatCelsius`) : « 80 % » en français, « 80% » en anglais, « %80 » en turc, « Вт » en russe.

Langues fournies : en-US (langue par défaut, `DefaultLanguage` du projet, repli quand la langue de Windows n'est pas fournie), fr-FR, es-ES, de-DE, it-IT, pt-BR, nl-NL, pl-PL, ru-RU, tr-TR, ja-JP, ko-KR, zh-Hans. Windows choisit d'après la liste des langues préférées de l'utilisateur. Les messages d'erreur renvoyés par le service (`error.message`) restent en anglais : le widget les cite après un préfixe traduit.

### Ajouter une langue

1. Copier `Strings/en-US/Resources.resw` dans `Strings/<balise BCP-47>/Resources.resw` (par ex. `Strings/sv-SE/`) et traduire chaque `<value>`. Garder toutes les clés et les marqueurs `{0}`, `{1}`, `{2}` ; les `<comment>` de la version anglaise décrivent chaque chaîne. Rester court pour les noms des tuiles de mode (environ 10 caractères : quatre tuiles sur 340 px de large).
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
