# Guide du développeur

[🇬🇧 English version](developer-guide.en.md) · [📚 Index de la documentation](README.md)

Ce guide décrit l'architecture de Canapé, le protocole entre ses composants, la façon de compiler et de tester (sous Linux sans matériel comme sur l'Ally), l'empaquetage, le widget, les traductions et les règles du projet.

## Sommaire

- [Architecture](#architecture)
  - [Processus, sessions et niveaux d'intégrité](#processus-sessions-et-niveaux-dintégrité)
  - [Crates](#crates)
  - [La bibliothèque de jeux](#la-bibliothèque-de-jeux)
    - [Installer les jeux possédés](#installer-les-jeux-possédés)
  - [La page Matériel](#la-page-matériel)
  - [Le service de l'intérieur](#le-service-de-lintérieur)
- [Protocole IPC](#protocole-ipc)
  - [Transport](#transport)
  - [Requêtes](#requêtes)
  - [Réponses](#réponses)
  - [Le statut](#le-statut)
  - [Types imbriqués](#types-imbriqués)
  - [Erreurs](#erreurs)
  - [Faire évoluer le protocole](#faire-évoluer-le-protocole)
- [Compiler et tester sous Linux](#compiler-et-tester-sous-linux)
  - [Service et shell avec le simulateur](#service-et-shell-avec-le-simulateur)
  - [Variables d'environnement](#variables-denvironnement)
  - [Captures d'écran avec Xvfb](#captures-décran-avec-xvfb)
- [Compiler et tester sous Windows](#compiler-et-tester-sous-windows)
- [Intégration continue](#intégration-continue)
- [Empaquetage](#empaquetage)
- [Le widget Game Bar](#le-widget-game-bar)
- [Traductions](#traductions)
- [Notes sur le matériel](#notes-sur-le-matériel)
- [Conventions](#conventions)
- [Travailler à deux sessions : l'Ally et le cloud](#travailler-à-deux-sessions--lally-et-le-cloud)

## Architecture

Canapé est fait de trois programmes qui tournent en même temps, plus un outil en ligne de commande. **Seul le service a des droits élevés** et touche au matériel ; l'interface et le widget ne font qu'envoyer des requêtes.

```mermaid
flowchart LR
    subgraph S1["Session 1 : utilisateur"]
        FSE["Expérience plein écran<br/>GamingHomeApp"]
        GB["Xbox Game Bar"]
        SHELL["canape-shell.exe<br/>medium IL, asInvoker<br/>identité du paquet Canape"]
        WIDGET["Widget Canapé<br/>UWP, AppContainer"]
        CTL["canape-ctl.exe"]
    end
    subgraph S0["Session 0 : services"]
        SVC["canape-service.exe<br/>CanapeService, LocalSystem"]
        STATE[("state.json")]
    end
    subgraph HW["Firmware et pilotes"]
        ATK["Firmware ASUS<br/>périphérique ATKACPI"]
        ADL["Pilote AMD<br/>atiadlxx.dll, ADL"]
    end
    FSE -->|"windows.gaming:///home"| SHELL
    GB --> WIDGET
    SHELL <-->|"JSON, une ligne par message<br/>named pipe canape"| SVC
    WIDGET <-->|"CreateFileFromAppW"| SVC
    CTL <--> SVC
    SHELL -.->|"FrameMetrics_Start"| ADL
    SVC -->|"DEVS / DSTS par IOCTL"| ATK
    SVC -->|"FPS, activité GPU"| ADL
    SVC --- STATE
```

### Processus, sessions et niveaux d'intégrité

| Processus | Session | Intégrité / compte | Identité | Démarré par |
|---|---|---|---|---|
| `canape-service.exe` | 0 | LocalSystem | aucune | Gestionnaire de services, au boot (démarrage automatique, redémarrage après plantage : 3 fois, 2 s d'attente) |
| `canape-shell.exe` | 1 | **medium IL** (`asInvoker`) | paquet sparse `Canape`, AUMID `Canape_<hash>!App` | Le FSE, avec `windows.gaming:///home` ; ou `shell:AppsFolder\…` |
| Widget | 1 | AppContainer (low IL) | paquet `Canape.Widget` | La Game Bar |
| `canape-ctl.exe` | 1 | celui du terminal | aucune | À la main |

Conséquences :

- Le FSE n'active l'app d'accueil qu'en **medium IL** : le shell doit rester `asInvoker` et ne rien demander de privilégié. Tout ce qui touche au matériel passe par le service.
- Le service, en **session 0**, n'a ni écran ni fenêtres : il ne peut pas savoir quel jeu est au premier plan (le shell le lui dit, `foreground_app`) et le pilote AMD refuse d'y démarrer le comptage des images (le shell le démarre, voir [Notes sur le matériel](#adl-depuis-la-session-0)).
- Le widget vit dans un AppContainer : il ouvre le pipe avec `CreateFileFromAppW`, et la DACL du pipe l'autorise explicitement.
- Le shell est **à instance unique** (mutex `Local\Canape.Shell`) : une deuxième activation ramène la fenêtre existante au premier plan et se termine. Le FSE active parfois l'accueil plusieurs fois au boot (voir [m1-report.md](m1-report.md)).

### Crates

```mermaid
flowchart BT
    core["canape-core<br/>protocole, réglages, banc d'essai"]
    autotdp["canape-autotdp<br/>contrôleur pur"]
    hw["canape-hw<br/>trait Device, ATKACPI, ADL, simulateur"]
    ipc["canape-ipc<br/>pipe Windows, socket Unix"]
    service["canape-service"]
    library["canape-library<br/>Steam, Epic, GOG, Xbox"]
    shell["canape-shell"]
    ctl["canape-ctl"]
    hw --> core
    ipc --> core
    service --> core
    service --> autotdp
    service --> hw
    service --> ipc
    shell --> core
    shell --> ipc
    shell --> library
    shell --> hw
    ctl --> core
    ctl --> ipc
```

| Crate | Type | Rôle | Fichiers clés |
|---|---|---|---|
| `canape-core` | bibliothèque | Protocole (`Request`, `Response`, `Status`), réglages (`AutoTdpSettings`, `PerformanceMode`, `Profile`, `ControllerMode`), courbes de ventilateurs (validation, préréglages), banc d'essai (`BenchPlan`, `BenchReport` et ses recommandations), journal minimal | `protocol.rs`, `settings.rs`, `fan.rs`, `bench.rs`, `log.rs` |
| `canape-autotdp` | bibliothèque | Contrôleur d'auto TDP : machine à états pure, nourrie d'un `Sample` par seconde, sans dépendance au système | `lib.rs` |
| `canape-hw` | bibliothèque | Trait `Device` ; backend Ally (Windows) au-dessus d'ATKACPI, d'ADL et de l'interface HID de la manette ; `MockDevice` pour le reste. Pour la session de l'utilisateur : l'écran (`display`) et le gyroscope (`motion` : trait `Gyro`, `MockGyro`, `motion::open()`) | `lib.rs`, `atk.rs`, `pad.rs`, `windows.rs`, `adl.rs`, `hid.rs`, `mock.rs`, `display.rs`, `motion.rs`, `gyro.rs` |
| `canape-ipc` | bibliothèque | Une `Connection` qui envoie et reçoit des lignes JSON ; named pipe sous Windows, socket Unix ailleurs ; ACL du pipe | `lib.rs`, `windows.rs`, `unix.rs`, `acl.rs` |
| `canape-service` | binaire | Service Windows : état, auto TDP, profils, banc d'essai, détection des conflits, serveur IPC, persistance | `handler.rs`, `server.rs`, `bench_run.rs`, `conflicts.rs`, `winsvc.rs` |
| `canape-library` | bibliothèque | Jeux installés : recherche Steam, Epic, GOG et Xbox, fusion et tri, décodage des jaquettes, jaquettes téléchargées, lancement, fichiers de cache. Jeux possédés et leur installation (Steam, Epic par Legendary). Analyseurs purs testés sous Linux ; les parties Windows (registre, lecteurs, `ShellExecuteW`, WinHTTP) derrière `cfg(windows)` | `lib.rs`, `steam.rs`, `vdf.rs`, `epic.rs`, `gog.rs`, `xbox.rs`, `legendary.rs`, `install.rs`, `tools.rs`, `online.rs`, `http.rs`, `cover.rs`, `cache.rs`, `launch.rs`, `win.rs` |
| `canape-shell` | binaire | App d'accueil Slint (rendu logiciel) : quatre sections (Jeux, Performance, Matériel, Réglages) et cinq sous-pages (Profils de jeu et Ajouter un jeu depuis Réglages, Banc d'essai depuis Performance, Visée au gyroscope et Boutons arrière et éclairage depuis Matériel), barre d'état (W, FPS, batterie, heure), graphes de puissance en direct, préréglages et courbe des ventilateurs, sélecteur de couleur d'accent, manette XInput, suivi du premier plan, 13 langues | `main.rs`, `link.rs`, `library.rs`, `addgames.rs` (la sous-page Ajouter un jeu), `shelves.rs` (composition des étagères, pure et testée : Récents, Installés, une par boutique (installés et à installer mélangés, Autres en dernier), À installer ; chaque étagère a son modèle et sa sélection, une jaquette décodée sert à toutes les étagères qui montrent le jeu), `backdrop.rs` (lueur de la couleur du jeu derrière la bibliothèque, et `fade_art` qui fond la grande image d'un jeu dans le fond), fiche du jeu (`GamePage` dans `library.slint` ; le profil s'affiche par l'exécutable que `learn_exe` retient dans `recent.json` (`Recent::exes`) quand un programme plein écran passe devant dans les 2 min suivant un lancement, `canape_library::is_store_program` écartant clients et lanceurs ; `open_detail` / `close_detail` : taille et temps de jeu tout de suite, grande jaquette et image large Steam décodées sur un thread puis libérées 400 ms après la fermeture), `grid.rs`, `clock.rs`, `hardware.rs`, `ui/*.slint` : `app.slint` (cadre, sections, aides), `kit.slint` (rangées, tuiles, réglettes, lignes de réglage partagées ; `Reveal` fait apparaître en fondu et glissé ce que crée un `if` ou un `for`, qui n'animent rien d'eux-mêmes : pages selon le sens de LB/RB ; `RevealLine` fait entrer une ligne de gauche à droite avec un délai selon son rang, pour les lignes du titre du jeu ; le fond de la bibliothèque, dessiné opaque à la taille de la fenêtre par `backdrop.rs` pour n'être que copié, passe d'une couleur à l'autre sur deux calques, `backdrop-a` et `backdrop-b` ; les jaquettes sont décodées à la taille d'une jaquette non sélectionnée, seule la sélectionnée est agrandie ; le rendu logiciel ne redessine que ce qui change, alors les animations touchent le moins de surface possible : une étagère ne défile qu'au bord, la jaquette sélectionnée grandit sur place sans pousser les autres, et le fond ne change qu'une fois la sélection posée, `BACKDROP_DELAY` ; il ne passe en fondu qu'après une pause, `BACKDROP_FADE_AFTER`, et change d'un coup pendant qu'on parcourt ; son image commence là où la lueur arrive, `Mask::x`), `fonts/` (Plus Jakarta Sans embarquée, licence OFL, chiffres tabulaires par défaut, espace élargie ; le cyrillique et le CJK retombent sur les polices système ; le widget lie les mêmes fichiers), une page par fichier (`library.slint`, `performance.slint`, `hardware.slint`, `settings.slint`, sous-pages dans `pages.slint`). Toutes les pages se parcourent aux flèches : chacune tient le focus dans un seul `FocusScope` et déplace elle-même sa rangée et sa colonne ; `App.arrow-focus` dit à la manette d'envoyer des flèches plutôt que Tab (`library::arrow_navigation`). X et Y arrivent en F2 et F3, `i18n.rs`, `report.rs`, `nav.rs` (`Navigator::quiet` : manette ignorée 2 s (`keys::FIRST_QUIET`) au premier passage au premier plan, boutons tenus ignorés jusqu'au relâchement), `keys.rs` (`KeyGate`, filtre des événements clavier de winit (`on_winit_window_event`) avec les mêmes règles : une touche qui se répète sans avoir été vue descendre était tenue avant le focus ; journalise chaque Entrée et Échap avec sa source, `keys: pad` ou `keys: keyboard`), `foreground.rs`, `history.rs`, `accent.rs`, `gyro.rs` (visée au gyroscope, voir [La page Matériel](#la-page-matériel)) |
| `canape-ctl` | binaire | Envoie une requête JSON, ou `watch` pour suivre le statut ; `--pretty` indente le JSON | `main.rs` |

Le shell utilise `canape-hw` pour ce qui appartient à la session de l'utilisateur : `arm_frame_metrics()` (démarrer le comptage des images du pilote AMD), l'écran, les boutons de l'Ally et le gyroscope. Il n'écrit jamais dans le firmware.

### La bibliothèque de jeux

`canape-library` trouve les jeux installés (et les jeux possédés, voir [Installer les jeux possédés](#installer-les-jeux-possédés)) et `canape-shell` les affiche (`library.rs`, `ui/library.slint`). Rien ne passe par le service : chercher, lancer et installer ne demandent aucun droit, et un jeu lancé depuis le shell doit tourner sous l'utilisateur, en medium IL, jamais élevé.

| Boutique | Où | Analyse | Lancement | Jaquettes |
|---|---|---|---|---|
| Steam | `HKCU\Software\Valve\Steam\SteamPath`, sinon `C:\Program Files (x86)\Steam` ; `steamapps\libraryfolders.vdf` puis chaque `appmanifest_*.acf` ; `userdata\*\config\localconfig.vdf` | `vdf.rs` : format texte KeyValues, tolérant (commentaires, `[$WIN32]`, jetons sans guillemets, fichiers tronqués). Installé = bit 4 de `StateFlags`. Outils écartés par appid et par nom (228980, Proton, Steam Linux Runtime, SteamVR) | `steam.exe -silent steam://rungameid/<appid>` (`steam::launch` : Steam démarre caché s'il ne tournait pas), l'URI seule sans `steam.exe` | `userdata\*\config\grid\<appid>p.*`, puis `appcache\librarycache\<appid>_library_600x900.jpg` (ancienne disposition) ou `<appid>\[<hash>\]library_600x900.jpg` (actuelle), puis `library_hero.jpg`, `header.jpg` |
| Epic | `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item` (JSON) | `epic.rs` : `AppCategories` doit contenir `games` ; DLC (`MainGameAppName` ≠ `AppName`) et `bIsIncompleteInstall` écartés | `com.epicgames.launcher://apps/<ns>%3A<item>%3A<app>?action=launch&silent=true`, jusqu'à ce que Legendary reprenne le jeu : après chaque scan, avec un compte Epic connecté, `adopt_epic_installs` (shell) passe chaque installation du lanceur (`epic::installs`, `InstallLocation`) à `install::epic_import` (`legendary import --skip-dlcs`), une tentative par jeu et par session ; `scan` écarte alors l'entrée du lanceur au profit de celle de Legendary | téléchargées (`online.rs`) |
| Epic par Legendary | `%USERPROFILE%\.config\legendary\installed.json`, seulement si `%LOCALAPPDATA%\Canape\tools\legendary.exe` existe (`Roots::legendary`) | `legendary.rs` : `parse_installed`, DLC (`is_dlc`) et titres vides écartés | `legendary.exe launch <app>`, dossier de travail `install_path` | téléchargées (`online.rs`) |
| GOG par gogdl | `%LOCALAPPDATA%\Canape\gog\installed.json`, le relevé par Canapé de ses installations gogdl (`Roots::gogdl`) | `gogdl.rs` : `scan` / `installed_game`, puis le `goggame-<id>.info` de chaque dossier (`parse_game_info`) ; un dossier disparu est sauté | l'exécutable de la `FileTask` principale, ses `arguments`, dossier de travail `workingDir` | téléchargées (`online.rs`) |
| GOG | `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>` | `gog.rs` : `gameName`, `exe`, `launchParam`, `workingDir` / `path` ; clés avec `dependsOn` (DLC) écartées | l'`exe` directement (dossier de travail `workingDir`, sinon `path`), sinon `goggalaxy://openGameView/<id>` | téléchargées (`online.rs`) |
| Xbox | `<lecteur>:\.GamingRoot` (sinon `<lecteur>:\XboxGames`), puis `<jeu>\Content\MicrosoftGame.config` et `appxmanifest.xml` | `xbox.rs` + `xml.rs` (attributs du premier élément trouvé). Nom de famille du paquet = `Name` + `_` + identifiant de l'éditeur, calculé (SHA-256 de l'éditeur en UTF-16LE, 8 octets, base 32 façon Crockford), vérifié contre `8wekyb3d8bbwe` et le `Canape_84zrj1c9kn010` de l'Ally | `shell:AppsFolder\<PFN>!<AppId>` | `Square480x480Logo` (ou sa variante `.scale-NNN`), `Square150x150Logo`, `StoreLogo` |
| Autres (`Source::Other`) | `%LOCALAPPDATA%\Canape\games.json` (`Roots::external`, fixé par le shell) | `external.rs` : `{"games": [{title, exe, args, dir, cover}]}` ou un tableau seul, marque d'ordre des octets tolérée ; id `other:` + 16 chiffres hexadécimaux du SHA-256 de `exe` en minuscules et de `args` (un émulateur listé une fois par ROM donne un id par jeu ; renommer garde l'historique) ; `dir` vaut par défaut le dossier de `exe` ; une `cover` relative est jointe au dossier de `games.json` et gardée seulement si le fichier existe. Un fichier illisible ne donne aucun jeu, est journalisé à chaque scan et n'est jamais réécrit | `Launch::Exe` par `ShellExecuteW` (un `.lnk`, un script ou un document marchent donc aussi), `Launch::Uri` quand `exe` a un schéma de deux caractères ou plus (`itch://…`) | sa `cover`, sinon téléchargée (`online.rs`, recherche SteamGridDB par titre) |

- **Fusion** (`merge`) : les lancements enregistrés par Canapé (`recent.json`) relèvent `last_played` ; une entrée par id, puis une par titre entre boutiques (lettres et chiffres, en minuscules : `HADES™` = `Hades`), en gardant la copie jouée en dernier, sinon Steam > GOG > Epic > Xbox > Autres ; tri par dernière partie puis par titre (`Sort::Recent`) ou par titre (`Sort::Title`).
- **Démarrage** : `library.json` (la dernière recherche brute) s'affiche avant l'ouverture de la fenêtre, puis un thread cherche en arrière-plan, réécrit le cache et poste le résultat à la boucle d'événements. Revenir sur la page relance une recherche au-delà de 60 s.
- **Jaquettes** : décodées sur un thread dédié (`zune-jpeg`, `png`, pur Rust) directement à la taille de la tuile en pixels physiques (filtre boîte), coins arrondis inclus dans l'image (le rendu logiciel ne découpe pas les images selon un rectangle arrondi). Seules les rangées visibles et une rangée de chaque côté sont demandées ; les jaquettes plus loin d'une rangée encore sont libérées. Les images paysage ou carrées s'affichent en entier sur une copie floutée et assombrie d'elles-mêmes. Mesuré sous Linux (release, 50 jeux, 1280×720 à l'échelle 1, mémoire privée `RssAnon`) : 2,7 Mo sans jeux, 6,9 Mo sur la bibliothèque avec ses 18 premières jaquettes, 10,8 Mo après avoir défilé jusqu'au bout et être revenu (glibc avec `MALLOC_ARENA_MAX=1` ; avec ses réglages par défaut, elle garde les tampons de décodage libérés : 9,1 / 14 Mo). Une jaquette coûte largeur × hauteur × 4 octets : environ 210 Ko en 1280×720, 470 Ko sur l'Ally à 150 %. À mesurer sous Windows.
- **Jaquettes téléchargées** (`online.rs`, `http.rs`) : après chaque scan, les jeux encore sans image passent par `online::download_missing` (60 au plus par scan, l'un après l'autre) : les jeux Steam essaient d'abord le CDN de la boutique Steam (`library_600x900.jpg`), puis, si `steamgriddb_key` est renseignée dans `shell.json`, SteamGridDB (`/grids/steam/<appid>`, ou `/search/autocomplete/<titre>` puis `/grids/game/<id>`, uniquement des images fixes portrait en JPEG/PNG). Les fichiers vont dans `%LOCALAPPDATA%\Canape\covers` sous le nom `<id, caractères non alphanumériques remplacés par _>.jpg|png`, écrits en `.part` puis renommés ; un jeu sans image reçoit un marqueur `.miss` vide, réessayé au bout de 7 jours. `online::attach` donne à chaque scan les jaquettes déjà téléchargées. Le transport est le trait `Http` : WinHTTP sous Windows (proxy système, redirections, délais de 5 à 15 s, 8 Mo maximum), rien ailleurs ; la logique est testée avec un faux transport. La clé n'est envoyée qu'en jeton bearer à `www.steamgriddb.com/api/v2`, jamais à un CDN (testé).
- **Navigation** : la grille est un seul `FocusScope` avec un index sélectionné ; les déplacements sont calculés en Rust (`grid.rs`, `Library.grid-move`). Tant que la bibliothèque a le focus, `gamepad.rs` envoie les flèches au lieu de Tab / Maj+Tab (`library::arrow_navigation()`), avec la même répétition. Vers le haut depuis la première rangée, on atteint la barre de filtres.
- **Lancement** : `canape_library::launch` appelle `ShellExecuteW` (COM initialisé sur le thread appelant) sur un thread éphémère, enregistre le lancement dans `recent.json`, affiche « Lancement… » 5 s, puis retrie.

> [!WARNING]
> **Pas encore essayé sous Windows.** Tout ce qui précède compile pour `x86_64-pc-windows-msvc` et les analyseurs sont testés sous Linux sur des fixtures (`crates/canape-library/tests/fixtures`), mais la lecture du registre, Steam / Epic / GOG sur une vraie machine, `ShellExecuteW` depuis le FSE et toute la partie Xbox (écrite d'après les formats documentés, sans jeu Xbox pour essayer) restent à vérifier sur l'Ally. Limite connue : les jeux de l'app Xbox installés hors d'un dossier `XboxGames` (dans `WindowsApps`) ne sont pas trouvés. `games.json`, la recherche des raccourcis et la lecture des `.lnk` ne sont pas essayés sur l'Ally non plus (l'analyseur est testé sur des raccourcis générés d'après le format publié, pas sur des raccourcis écrits par Windows).

**Jeux à ajouter depuis le shell** (`shortcut.rs`, `canape-shell/src/addgames.rs`, `AddGamesPage` dans `ui/pages.slint`, page 8 de `app.slint`, ligne 7 des Réglages) :

- `shortcut::folders` : `SHGetKnownFolderPath` pour `FOLDERID_Programs`, `FOLDERID_CommonPrograms`, `FOLDERID_Desktop`, `FOLDERID_PublicDesktop` ; `find` les parcourt (4 niveaux, 4 000 fichiers au plus) à la recherche de `.lnk`.
- `shortcut::parse` lit directement le format binaire Shell Link (sans COM) : en-tête et CLSID vérifiés, liste d'ID sautée, chemin de base local de la LinkInfo (décalages Unicode quand l'en-tête fait 0x24 octets ou plus, sinon ANSI lu en Latin-1) plus le suffixe commun, puis les chaînes (dossier de travail, arguments), puis la cible de l'`EnvironmentVariableDataBlock` (`%ProgramFiles%\…`) quand il n'y a pas de LinkInfo ; `expand` remplace les `%VARIABLES%`. Un raccourci sans chemin de fichier (apps Windows, raccourcis d'installeur « annoncés ») est ignoré. Les fichiers tronqués sont refusés sans panique (testé sur chaque préfixe).
- `could_be_game` : un `.exe`, hors de `%SystemRoot%`, ni client de boutique (`is_store_program`) ni Canapé, sans `uninst`, `setup`, `install`, `update`, `readme`, `manual`, `website`, `support`, `crashreport`, `redist`… dans son nom de fichier ou de raccourci ; la cible doit exister. Les doublons (même cible et mêmes arguments) ne sont gardés qu'une fois, triés par nom.
- `addgames.rs` : à l'ouverture, un thread lit `games.json` et les raccourcis ; la liste est `games.json` (marqués ajoutés) plus les raccourcis qui n'y sont pas et dont ni le titre (`title_key`) ni le programme ne correspondent à un jeu de boutique. A écrit `games.json` aussitôt (`external::save`, par un fichier `.tmp`) après l'avoir relu (un fichier illisible est refusé, affiché en jaune), puis `library::rescan()` ; un scan déjà en cours est suivi d'un autre (`State::rescan`).
- `Library.store-name` traduit « Other » (« Autres jeux ») ; `StoreLogo` dessine une manette pour lui (`fill-rule: evenodd` pour les trous).

**Fausse bibliothèque pour le développement** : `python3 tools/fake-library.py <dossier>` en fabrique une avec 14 jeux inventés répartis sur les quatre boutiques, des jaquettes générées (ImageMagick), des dates de jeu, et les jaquettes Epic et GOG placées comme si elles avaient été téléchargées ; le script affiche les deux variables à définir (`LOCALAPPDATA` et `CANAPE_LIBRARY_FIXTURE`). Dans un build de debug, `CANAPE_LIBRARY_FIXTURE=<dossier>` remplace les vraies boutiques par `<dossier>/steam` (un dossier Steam), `<dossier>/epic` (manifestes), `<dossier>/gog.json` (un tableau JSON de `{"key": …, "values": {"gameName": …, "exe": …}}`, comme le registre) et `<dossier>/xbox` (un dossier `XboxGames`), plus `<dossier>/games.json` (deux jeux de l'étagère Autres jeux, dont un avec sa jaquette dans `<dossier>/covers`) et, pour Ajouter un jeu, les raccourcis de `<dossier>/shortcuts` (cibles non vérifiées). Le script ajoute 9 jeux possédés mais pas installés (4 Steam, 3 Epic, 2 GOG) dans `<dossier>/owned.json`, au format du cache, avec leurs jaquettes dans `covers-fixture` : c'est la liste du mode À installer. Rien n'est lancé (le lancement est seulement affiché), et les fichiers de cache deviennent `library-fixture.json`, `recent-fixture.json` et `owned-fixture.json`. Les builds release ignorent la variable.

#### Installer les jeux possédés

L'étagère **À installer** de la bibliothèque liste les jeux Steam, Epic et GOG possédés mais pas installés ; A les installe sans ouvrir l'interface de Steam, le lanceur Epic ni GOG Galaxy. L'analyse de départ est dans [`game-install-study.md`](game-install-study.md) ; Xbox n'est pas couvert.

| Module | Rôle |
|---|---|
| `install.rs` | `Install` (`Steam { appid }`, `Epic { app }` ou `Gog { id }`, sérialisé avec une étiquette `store`) ; `install::run`, bloquant (à lancer sur son propre thread), qui rend des `Event` : `Progress(0..1)`, `Done`, `Failed(message)`. Les commandes Legendary : `epic_account` (`status --json --offline`), `epic_owned` (`list --json`), `epic_login` (`auth`). Les commandes gogdl : `gog_login`, `gog_credentials`, `gog_info`, `run_gogdl` (voir plus bas). |
| `gogdl.rs` | GOG par gogdl : `login_url`, `parse_code`, `parse_credentials`, `owned_url` / `parse_owned_page` / `fetch_owned`, `parse_user`, `parse_info`, `parse_progress`, `parse_game_info`, le relevé des installations (`Installed`, `record_install`, `scan`). |
| `steam.rs` | `steam_id` : le compte `MostRecent` de `config/loginusers.vdf`, sinon le `Timestamp` le plus récent. `owned_url` : `IPlayerService/GetOwnedGames/v1` avec `include_appinfo` et `include_played_free_games`. `parse_owned` (outils écartés comme pour la recherche). `parse_download` / `download` : l'état d'une installation d'après son manifeste, cherché dans tous les dossiers de bibliothèque. |
| `legendary.rs` | `config_dir`, `scan` / `parse_installed` (voir le tableau plus haut), `parse_list` (les jeux gérés par une autre boutique, `ThirdPartyManagedApp` ou `ThirdPartyManagedProvider`, sont écartés ; l'image `DieselGameBoxTall` en `https://` devient le lien de jaquette, demandée en 600×900), `parse_progress`, `parse_account`. |
| `tools.rs` | Les téléchargements de Legendary et de gogdl (voir plus bas). |
| `disk.rs` | Le dossier des jeux et la place libre : `free_space` (`GetDiskFreeSpaceExW` sur le dossier ou son plus proche parent existant), `fixed_drives` (`GetLogicalDrives`, `GetDriveTypeW` = `DRIVE_FIXED`), `parents` (les dossiers qui contiennent des jeux installés, le plus rempli d'abord), `default_games_dir`, `choices` / `next_choice` (la liste de la ligne des Réglages), `shortage` (le message de refus). |
| `lib.rs` | `Launch::Install { install }` pour un jeu possédé non installé (`launch` le refuse) ; `not_installed` : les possédés moins les installés, par id ou par titre normalisé. |
| `online.rs` | `download_missing` reçoit les liens d'images donnés par les boutiques (`art` : id → URL) et les essaie avant le CDN Steam et SteamGridDB. `Http::get_limited` fixe une autre limite de taille que les 8 Mo de `get`. |
| `canape-shell/src/library.rs` | Étagère `Kind::ToInstall`, `refresh_owned` / `fetch_owned`, `install` / `install_event`, `epic_login`, `gog_login` / `gog_check_clipboard`. `clipboard.rs` lit le texte du presse-papiers. |
| `ui/library.slint`, `ui/pages.slint`, `ui/app.slint` | Sélecteur Installés / À installer (curseur de la barre : modes 0–1, filtres 2–6, ordres 7–8), barre de progression sur la tuile, état vide avec le bouton de connexion Epic, ligne des Réglages, aide « A Installer ». |

- **Liste des possédés** (`refresh_owned`) : au démarrage, puis en arrivant sur l'étagère À installer si la dernière liste a plus de 10 min (`OWNED_REFRESH`), et forcée après une connexion Epic ou GOG. Sur un thread : Steam si `steam_api_key` est renseignée dans `shell.json` (Windows uniquement : SteamID local, `GetOwnedGames` par WinHTTP, `parse_owned`), puis Epic si `legendary.exe` est présent et qu'un compte est connecté, puis GOG si gogdl est présent et que `gog\auth.json` contient une session. Le résultat est écrit dans `owned.json` (même format que `library.json`) et affiché ; les jaquettes manquantes sont ensuite téléchargées avec les liens d'Epic et de GOG, puis la liste est republiée. Les erreurs vont dans `shell.log` et dans l'en-tête pendant 10 s (`show_error`, `ERROR_SHOWN`) ; après une erreur, `owned.json` n'est pas écrasé. `rebuild` place `not_installed(owned, all)`, trié par nom, sur la dernière étagère.
- **Steam** (`run_steam`) : `steam.exe -silent steam://install/<appid>` par `ShellExecuteW` (l'URI seule si `steam.exe` manque). `-silent` démarre Steam dans la zone de notification s'il ne tourne pas ; s'il tourne, l'URI va au client. Le manifeste `appmanifest_<appid>.acf` est ensuite relu toutes les 2 s : `StateFlags` à 4, ou bit 4 avec `BytesDownloaded >= BytesToDownload > 0`, vaut installé ; sinon la part `BytesDownloaded / BytesToDownload` (0 tant que Steam prépare). Pas de manifeste au bout de 10 min (`STEAM_START`, l'utilisateur n'a pas confirmé) : `Steam did not start the download` ; un manifeste qui disparaît après être apparu : `cancelled in Steam`.
- **Dossier des jeux et place libre** (`disk.rs`, `State::games_dir` dans le shell) : `games_folder` de `shell.json` s'il est renseigné, sinon le dossier qui contient le plus de jeux installés par Legendary (`legendary::install_paths`), gogdl (`installed.json`) ou le lanceur Epic (`epic::installs`), sinon `Games` sur le lecteur fixe qui a le plus de place libre, sinon `%USERPROFILE%\Games`. Réglages → Dossier des jeux parcourt `next_choice` (les dossiers qui contiennent déjà des jeux, puis `<lecteur>\Games` pour chaque lecteur fixe) et enregistre le choix. `install::size_needed` demande à la boutique avant une installation : `legendary info <app> --json` (`manifest.disk_size`, `legendary::parse_info_size`) ou `gogdl info` (`disk_size`) ; Steam : `None`, sa fenêtre l'affiche. Ouvrir la fiche d'un jeu Epic ou GOG à installer l'appelle sur un thread, une fois par jeu et par session (`State::sizes`, `fetch_size`), et remplit `DetailView.size-tenths` ; `free-tenths` et `folder` viennent de `disk::free_space`. La fiche les compare (`GamePage.short`) : si le jeu ne rentre pas, la ligne sous le bouton le dit en jaune et `act()` n'appelle pas `launch`. `run_legendary` et `run_gogdl` revérifient (`check_space`, `disk::shortage`) juste avant de télécharger et échouent avec `not enough space in <dossier>: X GB needed, Y GB free` ; Legendary et gogdl refusent aussi d'eux-mêmes. Tailles en Go décimaux (10⁹ octets), comme le reste de la fiche.
- **Epic** (`run_legendary`) : `legendary install <app> -y --skip-sdl --base-path <dossier des jeux>` (le jeu arrive dans `<dossier des jeux>\<son dossier>`) sans console (`CREATE_NO_WINDOW`), stdin fermé, stdout ignoré. La progression vient des lignes `= Progress: 12.34%` de stderr ; si le code de sortie n'est pas 0, la dernière autre ligne non vide devient le message d'erreur.
- **Fin** : `Event::Done` relance une recherche ; le jeu installé quitte la liste À installer puisqu'il porte le même id (`steam:<appid>`, `epic:<app>`, `gog:<id>`). `Event::Failed` écrit `<titre>: <message>` dans `Library.install-error`, effacé au début de l'installation suivante.
- **Connexion Epic** (`epic_login`) : télécharge Legendary s'il manque, lance `legendary auth` avec sa console visible (sans WebView2, Legendary y demande un code), attend la fin puis force la liste. Hors Windows, une erreur.
- **GOG** (`run_gogdl`) : `gogdl info`, la vérification de la place, puis `gogdl download` dans le dossier des jeux, stdout et stderr redirigés (stdout lu sur son propre thread). La progression vient de stderr ; si le code de sortie n'est pas 0, la dernière ligne de stdout de gogdl (son refus faute de place), sinon sa dernière ligne `ERROR`, devient le message. Un succès sans `goggame-<id>.info` dans le dossier (un téléchargement annulé sort avec 0) est un échec ; sinon l'installation est notée.
- **Légèreté** : rien ne tourne en permanence. Legendary et gogdl ne tournent que pour une demande de liste (`status` puis `list` pour Legendary, `auth` pour gogdl, seulement quand la liste À installer est rafraîchie), une connexion ou une installation. Le presse-papiers n'est lu que pendant que la page de connexion GOG est ouverte (10 min au plus).

**Téléchargement de Legendary** (`tools.rs`) : `fetch_legendary` demande la dernière publication à `api.github.com/repos/derrod/legendary/releases/latest` et prend l'asset `legendary.exe`. Il refuse un lien hors de `https://github.com/derrod/legendary/releases/download/`, et un asset sans `digest` `sha256:` de 64 caractères hexadécimaux. Le fichier (80 Mo au plus) est téléchargé, son SHA-256 calculé (`sha256.rs`) et comparé ; s'il correspond, il est écrit en `.part` puis renommé dans `%LOCALAPPDATA%\Canape\tools`. Un fichier qui ne correspond pas n'est jamais écrit (testé avec un faux transport). Legendary (GPL-3) reste un programme séparé que Canapé lance : rien n'est embarqué dans le dépôt ni dans l'installation.

**gogdl** (`gogdl.rs`, `install.rs`, `tools.rs`) : heroic-gogdl, l'outil de téléchargement GOG libre (GPL-3, lancé comme programme séparé, jamais embarqué). Son README prévient que son interface en ligne de commande n'est « pas faite pour les humains » et sert à d'autres programmes : les commandes et sorties ci-dessous ont été lues dans le code source de la version fixée (1.3.1, `gogdl/args.py`, `cli.py`, `auth.py`, `dl/managers/v2.py`, `dl/progressbar.py`, `imports.py`), et sont à revérifier avant de changer de version. Chaque commande tourne sans console, avec `GOGDL_CONFIG_PATH=%LOCALAPPDATA%\Canape\gog` (ses manifestes vont dans `gog\heroic_gogdl\manifests` au lieu de `%APPDATA%\heroic_gogdl`) et `--auth-config-path %LOCALAPPDATA%\Canape\gog\auth.json` avant la commande.

| Commande | Usage dans Canapé | Sortie lue |
|---|---|---|
| `auth --code <code>` | Connexion (`install::gog_login`) : gogdl échange le code contre des jetons (`auth.gog.com/token`, `grant_type=authorization_code`, client public de GOG Galaxy) et les écrit dans `auth.json` | stdout : les jetons en JSON, `{"error": true}` si refusé (`parse_credentials`) |
| `auth` | Avant chaque liste des possédés (`gog_credentials`) : les jetons actuels, rafraîchis avec le `refresh_token` une fois expirés | stdout : `{"access_token", "user_id", "refresh_token", "expires_in", "loginTime", …}`, `null` si personne n'est connecté ou si le rafraîchissement a échoué |
| `info <id> --platform windows --lang en-US --skip-dlcs` | Avant une installation (`gog_info`) | stdout : `{"size": {"*": {"download_size", "disk_size"}, "en-US": {…}, …}, "folder_name", "buildId", "languages", "dependencies", …}` ; `parse_info` additionne `*` (fichiers communs à toutes les langues) et `en-US` (sinon la seule autre langue) |
| `download <id> --platform windows --lang en-US --skip-dlcs --path <dossier des jeux>` | L'installation (`run_gogdl`) | stderr, chaque seconde : `[PROGRESS] INFO: = Progress: 42.15 2790066118/6619375133, Running for: …` (sans signe `%`, contrairement à Legendary ; `parse_progress`). Place insuffisante : `Unable to proceed, Not enough disk space` sur stdout, code de sortie 2. Le jeu arrive dans `<dossier des jeux>\<folder_name>`, qui contient alors `goggame-<id>.info` |

Pas utilisées : `launch` (Canapé lance l'exécutable lui-même, voir plus bas), `import`, `save-sync`, `redist` (les dépôts de dépendances propres au jeu sont téléchargés dans son dossier par `download`, mais personne ne lance leurs installateurs : limite connue).

- **Connexion** (`State::gog_login` dans le shell) : télécharge gogdl s'il manque, ouvre `gogdl::login_url()` (`auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https://embed.gog.com/on_login_success?origin=client&response_type=code&layout=client2`) dans le navigateur par défaut par `ShellExecuteW`, puis lit le presse-papiers chaque seconde pendant 10 min (`GOG_POLL`, `GOG_WAIT`, `clipboard.rs` : `OpenClipboard` / `CF_UNICODETEXT`). La redirection est imposée par GOG vers `embed.gog.com` : aucun serveur local ne peut l'attraper, l'utilisateur copie l'adresse de la page d'arrivée. `gogdl::parse_code` n'accepte qu'un texte contenant `on_login_success` et un paramètre `code=` en caractères sûrs pour une URL ; le texte du presse-papiers au départ est ignoré, pour ne pas reprendre une ancienne adresse. A pendant l'attente lit le presse-papiers tout de suite, sinon rouvre la page. `Library.gog-waiting` pilote l'état « En attente de l'adresse GOG… » et sa consigne.
- **Liste des possédés** : `gog_credentials`, puis par WinHTTP avec le jeton en bearer `embed.gog.com/userData.json` (`parse_user` : `username`, sinon le `user_id`) et `embed.gog.com/account/getFilteredProducts?mediaType=1&sortBy=title&page=N` page après page (`fetch_owned`, 50 pages au plus), en gardant les produits `worksOn.Windows` et `isGame` ; l'`image` (`//images-N.gog-statics.com/<hash>`) devient le lien d'image `https://…<hash>_392.jpg` (paysage, affichée en entier sur sa copie floutée ; suffixe encore à vérifier sur les serveurs de GOG).
- **Jeux installés** (`gogdl::scan`, `Roots::gogdl`) : gogdl ne tient pas de liste de ses installations, donc `run_gogdl` note `{id, path, size}` dans `%LOCALAPPDATA%\Canape\gog\installed.json` une fois `goggame-<id>.info` présent dans le dossier. Le scan lit ce fichier dans chaque dossier noté (`parse_game_info`) : la tâche de jeu `FileTask` principale donne l'exécutable (`path`, relatif au dossier), ses `arguments` et son dossier de travail (`workingDir`, sinon le dossier du jeu). L'id est `gog:<id>`, le même que pour une installation Galaxy du jeu : `merge` n'en garde qu'une.
- **Téléchargement de gogdl** (`tools::fetch_gogdl`) : contrairement à Legendary, la version est fixée : `GOGDL_URL` (`…/heroic-gogdl/releases/download/v1.3.1/gogdl_windows_x86_64.exe`) et `GOGDL_SHA256` (`1866e0ea…2a4c5f`, l'empreinte que GitHub affiche pour l'asset, vérifiée sur un téléchargement du fichier). L'exécutable est enregistré sous `gogdl-1.3.1.exe` : changer de version télécharge la nouvelle. Même écriture vérifiée que pour Legendary (`fetch_checked`).

**Développement** : avec `CANAPE_LIBRARY_FIXTURE`, deux faux lecteurs (`C:\` avec 21,4 Go libres, `D:\` avec 48,2 Go, `FIXTURE_DRIVES`) remplacent les vrais et la taille d'un jeu à installer est la `size` de son entrée dans `owned.json` (Lumen Drift, 63,1 Go, ne rentre pas) ; la liste des possédés vient de `<fixture>/owned.json`, le cache devient `owned-fixture.json`, rien ne va en ligne, et A simule une installation d'une minute (120 pas de 0,5 s) puis relance la recherche. Le faux jeu n'étant pas réellement installé, il reste dans la liste. Les captures `library-install.png` montrent cette installation simulée.

> [!WARNING]
> **Rien de tout cela n'a été essayé sous Windows.** Le code compile pour `x86_64-pc-windows-msvc` ; les analyseurs (`GetOwnedGames`, manifestes en cours de téléchargement, `legendary list` / `status`, sa progression, la publication GitHub) et la vérification du SHA-256 sont testés sous Linux sur des réponses enregistrées, et l'interface avec la fausse bibliothèque. Restent à vérifier sur l'Ally : l'appel à l'API Web Steam, `steam.exe -silent steam://install` et sa fenêtre à la manette, le téléchargement de `legendary.exe` par WinHTTP, `legendary auth` / `install` / `launch` depuis le shell et le FSE ; pour GOG, tout : le téléchargement de gogdl, le navigateur par-dessus le FSE et la lecture du presse-papiers, `gogdl auth` / `info` / `download`, les réponses web de GOG (les fixtures GOG de `tests/fixtures/gog` ont été écrites d'après le code source de gogdl 1.3.1 et la forme connue des réponses de GOG, pas enregistrées : les serveurs de GOG ne sont pas joignables depuis la session cloud), l'image `_392.jpg` et le lancement d'un jeu d'après son `goggame-<id>.info` ; pour le dossier des jeux, `GetDiskFreeSpaceExW` / `GetLogicalDrives` sur l'Ally, `legendary info --json` et `--base-path`, et les tailles comparées à ce que les boutiques installent vraiment.

### La page Matériel

La page 3 (`ui/hardware.slint`) ne fait qu'envoyer des requêtes : le service valide et écrit, et la page se redessine au statut suivant.

| Contrôle | Callback (`main.rs`) | Requête |
|---|---|---|
| Tuiles des ventilateurs | `set-fan-preset(key)` | `""` (Automatique) : `reset_fan_curves` ; `silent`, `balanced`, `max-cooling` : un `set_fan_curve` par ventilateur, CPU puis GPU, avec `FanPreset::curve()` (`canape-core/src/fan.rs`) |
| Pastilles de la limite de FPS | `set-fps-limit(fps)` | `set_fps_limit`, `0` → `null` (Aucune) ; pastilles 30, 40, 45, 60, 90, 120 |
| Garder ces réglages pour ce jeu | `save-hardware-profile()` | `save_profile` pour `status.foreground` : le profil existant (ou un nouveau avec `status.auto_tdp`) avec `fan_curves: Some(status.fan_curves)` (un ensemble vide veut dire « les courbes du firmware » pour ce jeu), `fps_limit: status.fps_limit`, `refresh_hz` (la fréquence actuelle de l'écran) et `gyro: Some(status.gyro)` ; aussi **X** sur la page gyroscope |
| Puces de l'écran | `set-refresh(hz)` | aucune : le shell change l'écran lui-même (`screen.rs`, `canape_hw::display::Screen`) et enregistre `refresh_hz` dans `shell.json` |
| Tuiles de la manette | `set-controller(mode)` | `set_controller_mode` |
| Puces de la limite de charge | `set-charge-limit(percent)` | `set_battery_charge_limit` ; puces 60, 80, 100 |
| Ligne Visée au gyroscope | `open-gyro()` | aucune : ouvre la page 6, la sous-page Visée au gyroscope (`GyroPage` dans `pages.slint`) |
| Lignes de la page gyroscope | `set-gyro-activation(nom)`, `change-gyro-sensitivity(pas)`, `toggle-gyro-roll()`, `toggle-gyro-invert()` | `set_gyro` avec `status.gyro` modifié (`update_gyro`) |
| Ligne Boutons arrière et éclairage | `open-controller()` | aucune : ouvre la sous-page 7, `ui/controller.slint` (B revient à la page Matériel) |
| Lignes M1 / M2 (sous-page) | `set-back-button(button, index)` | `set_back_button` avec `ButtonTarget::ALL[index]`, envoyé à chaque gauche / droite |
| Rétablir les boutons d'origine (sous-page) | `reset-back-buttons()` | `reset_back_buttons` |
| Ligne Luminosité (sous-page) | `set-lighting(level, color)` | `set_lighting` : Par défaut → `null` ; Désactivé, Faible, Moyenne, Forte → niveaux 0 à 3 avec la couleur actuelle |
| Pastilles de couleur (sous-page) | `set-lighting(level, color)` | `set_lighting` avec cette couleur (la première pastille, un anneau, `color: null` : suivre l'accent), au niveau affiché, Forte si les anneaux étaient éteints ou sur Par défaut |

- `hardware.rs` : `preset_name()` traduit `status.fan_curves` en tuile sélectionnée : `""` si vide (courbes du firmware), la clé d'un préréglage si chaque courbe personnalisée lui est égale, `custom` sinon. `curve_paths()` transforme la courbe du CPU en commandes `Path` de la ligne et de l'aire du cadre (viewbox 90 × 100 : une unité par °C de 20 à 110 °C, une par pour cent), prolongée à plat avant le premier point et après le dernier. Les deux sont couvertes par des tests unitaires.
- `link.rs` (`status_view`) : `cpu_fan_rpm` / `gpu_fan_rpm` (−1 si inconnus), le nom du préréglage, les chemins de la courbe du CPU (vides sans courbe CPU personnalisée), `fps_limit` (0 pour aucune), `controller_mode` (`""` tant que Canapé ne l'a pas réglé, donc aucune tuile sélectionnée) et `battery_charge_limit` (0 tant que Canapé ne l'a pas réglée, de même) ; `m1` / `m2`, l'indice de la cible de chaque bouton arrière dans `ButtonTarget::ALL` (0 = sa propre touche). Les noms de la page manette (`target-names` dans `controller.slint`) suivent cet ordre ; un test de `link.rs` vérifie qu'ils sont aussi nombreux. `lighting-level` (−1 sans `lighting`) et `lighting-color` (`""` quand elle suit l'accent). `tdp_floor()` / `tdp_ceiling()` lisent `tdp_min_watts` / `tdp_max_watts` (repli sur `MIN_WATTS` 5 et `MAX_WATTS` 35 dans `main.rs`) et bornent tous les réglages de puissance du shell.
- La page Profils et le bouton **Profil du jeu** du banc d'essai conservent les `fan_curves`, `fps_limit`, `refresh_hz` et `gyro` d'un profil existant.
- **Visée au gyroscope** (`gyro.rs`, étude dans [`gyro-study.md`](gyro-study.md)) : le service ne fait que stocker les réglages (`GyroSettings`, globaux dans `state.json` et par profil) et pousse ceux en vigueur dans `status.gyro` ; le shell fait le reste, car `SendInput` n'atteint que le bureau de sa propre session. `link.rs` passe chaque statut à `gyro::follow(réglages, foreground.is_some())`, et des réglages `Off` quand le service est perdu ; `buttons.rs` passe les palettes à `gyro::paddle(enfoncée)`, quoi que dise `ally_buttons`. Un thread attend sur une variable de condition que la visée soit voulue (`Trigger::active` : palette tenue, basculée, ou toujours, et un jeu devant), ouvre le capteur une fois (`canape_hw::motion::open()`, journalisé en `gyro: …`), demande un intervalle de 10 ms, puis toutes les 10 ms lit la vitesse angulaire, la convertit en comptes de souris (`Aim::step`) et les envoie par `SendInput(MOUSEEVENTF_MOVE)`. Tant que le shell est devant, rien ne bouge et il revérifie toutes les 250 ms. Retour à l'intervalle par défaut du capteur quand la visée s'arrête. `Aim` : zone morte de 0,75 °/s retranchée de chaque axe, 50 ms au plus par lecture, fractions gardées d'une lecture à l'autre ; tourner à gauche (positif autour de Y, ou de Z avec `roll`) va à gauche, pencher le haut vers l'arrière (positif autour de X) monte. `Aim` et `Trigger` sont testés.
- **Fréquence de l'écran** (`screen.rs`) : les modes d'affichage appartiennent à la session de l'utilisateur, c'est donc le shell, pas le service, qui les change. `canape_hw::display::Screen::internal()` retient le chemin actif dont la technologie de sortie est interne ou DisplayPort intégré (`QueryDisplayConfig`, puis le nom GDI de la source), `rates()` liste les fréquences à la résolution et à la profondeur actuelles (`EnumDisplaySettingsExW`), `set_refresh()` appelle `ChangeDisplaySettingsExW` avec `CDS_UPDATEREGISTRY`. Le service ne fait que stocker `Profile::refresh_hz` ; à chaque statut, le shell cherche la fréquence du profil actif et `screen::follow` ne change l'écran qu'au changement de profil (`Follow::step`, testé) : entrer dans un profil mémorise la fréquence remplacée, en sortir revient à la fréquence choisie sur la page (`refresh_hz` de `shell.json`, 0 = aucune) ou à celle mémorisée. La page relit la fréquence actuelle tous les 5 statuts. Hors Windows, `CANAPE_MOCK_SCREEN=60,120` simule un écran pour les captures.
- **Boutons de l'Ally** (`buttons.rs`, Windows) : un thread lance `canape_hw::listen_buttons`, qui ouvre la même interface HID que le mode de la manette (VID `0x0B05`, PID `0x1ABE`/`0x1B4C`, rapport de fonctionnalité `0x5A`), envoie le rapport de réveil, puis lit les rapports d'entrée : code 166 Command Center, 56 ou 147 Armoury, 165 / 0 palettes arrière enfoncées / relâchées, 236 ignoré (`pad::parse_input`, testé). Armoury → `instance::bring_to_front()` (un appui Alt synthétique, puis `SetForegroundWindow`), ou la section Jeux si Canapé est déjà devant ; Command Center → Win+G (`SendInput`). Les codes inconnus sont journalisés. Après une erreur, le thread réessaie toutes les 5 s, toutes les 60 s quand aucune interface n'existe. `ally_buttons: false` dans `shell.json` désactive les actions.

> [!WARNING]
> Toutes les écritures derrière cette page sont **non testées sur l'appareil** (voir [Notes sur le matériel](#notes-sur-le-matériel)). La page et son câblage ne sont testés qu'avec `MockDevice`.

### Le service de l'intérieur

```mermaid
flowchart TB
    subgraph Threads
        TICK["thread tick<br/>toutes les secondes"]
        ACCEPT["thread accept"]
        CLIENT["un thread par client"]
    end
    H["Handler<br/>derrière un Mutex"]
    SUBS["abonnés<br/>Vec de Writer"]
    TICK -->|"tick(), conflits toutes les 5 s"| H
    TICK -->|"statut"| SUBS
    ACCEPT -->|"nouvelle connexion"| CLIENT
    CLIENT -->|"handle(requête)"| H
    CLIENT -->|"après subscribe"| SUBS
```

- `Handler` (`handler.rs`) contient tout l'état et traite les requêtes, indépendamment du transport ; il est testé avec `MockDevice`.
- Le thread **tick** appelle `Handler::tick()` chaque seconde (mesures, banc d'essai, auto TDP), cherche les programmes en conflit tous les 5 ticks, enregistre `state.json` si des watts appris ou un banc d'essai ont changé l'état, puis pousse le statut aux abonnés.
- Chaque **client** a son thread. Une requête qui modifie l'état est journalisée (`SetTdp { watts: 12 } -> Ok`), suivie d'un enregistrement de `state.json` si la réponse est `ok`, puis d'un statut poussé à tous les abonnés.
- Arguments de `canape-service.exe` :

| Argument | Effet |
|---|---|
| *(aucun)* | Lancé par le gestionnaire de services ; lancé à la main sous Windows, se replie sur `--console`. |
| `--console` | Sert le pipe (ou le socket) au premier plan, sur tout OS ; journal sur stderr. |
| `--stdio` | Lit des requêtes JSON sur stdin et répond sur stdout, sans thread tick (ni auto TDP, ni statut poussé). Pour des essais rapides. |
| `install` / `uninstall` | Windows, administrateur : enregistre (et démarre) ou supprime le service `CanapeService`. |

## Protocole IPC

Les types sont définis dans [`crates/canape-core/src/protocol.rs`](../crates/canape-core/src/protocol.rs). Le widget C# reproduit la même chose à la main dans `widget/CanapeWidget/CanapeLink.cs`.

### Transport

| | Windows | Linux, macOS (développement) |
|---|---|---|
| Canal | Named pipe `\\.\pipe\canape` | Socket Unix : `$CANAPE_SOCKET`, sinon `$XDG_RUNTIME_DIR/canape.sock`, sinon `<tmp>/canape.sock` |
| Format | Un objet JSON par ligne (UTF-8, terminé par `\n`) | idem |
| Sécurité | DACL `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)(A;;GRGW;;;<SID du widget>)S:(ML;;NW;;;LW)` : contrôle total pour SYSTEM et les administrateurs, lecture / écriture pour l'utilisateur interactif et **le seul AppContainer du widget Canapé**, étiquette d'intégrité basse pour que le widget puisse écrire. Si le SID du widget ne peut pas être dérivé, `AC` (tous les AppContainers) le remplace. Le service journalise lequel au démarrage (`pipe: …`). Clients distants refusés, première instance exclusive (personne d'autre ne peut servir ce nom). Voir [ACL du pipe](#acl-du-pipe). | Droits du fichier socket |

Les messages sont des enums Serde à étiquette interne : le champ `type` donne la variante en `snake_case`, les autres champs sont à côté. Un `Option` vide s'écrit `null`.

**Déroulement d'une connexion**

```mermaid
sequenceDiagram
    participant C as Client
    participant S as Service
    C->>S: {"type":"hello","protocol_version":1}
    S-->>C: {"type":"hello","protocol_version":1}
    C->>S: {"type":"set_tdp","watts":12}
    S-->>C: {"type":"ok"}
    C->>S: {"type":"subscribe"}
    S-->>C: {"type":"status", …}
    loop environ chaque seconde, et après chaque changement
        S-->>C: {"type":"status", …}
    end
    Note over C,S: Après subscribe, le service ne lit plus rien sur cette connexion
```

- Chaque client **devrait** commencer par `hello`. Le service ne l'impose pas, mais refuse une version différente de `1`.
- Une requête reçoit exactement une réponse, dans l'ordre.
- Après `subscribe`, la connexion ne fait plus que recevoir. Raison : sous Windows, les entrées / sorties d'un pipe synchrone sont sérialisées sur le même objet fichier ; une lecture en attente bloquerait les écritures du thread tick (bug trouvé en M2, voir [m2-report.md](m2-report.md)). Les clients utilisent donc **une connexion d'abonnement et une ou plusieurs connexions de commandes** (le shell en garde une ouverte, le widget en ouvre une par commande).
- Une ligne qui n'est pas du JSON valide ferme la connexion (ligne `client error: …` dans le journal) ; seul le mode `--stdio` renvoie une `error`.
- Le client Windows réessaie pendant 2 s si toutes les instances du pipe sont occupées.

### Requêtes

| Requête | Champs | Réponse | Effet |
|---|---|---|---|
| `hello` | `protocol_version: u32` | `hello` ou `error` | Vérifie la version (actuelle : `1`). |
| `get_status` | — | `status` | Statut courant. |
| `subscribe` | — | `status`, puis un `status` environ chaque seconde | La connexion devient « push » seulement. |
| `set_performance_mode` | `mode` : `silent` \| `performance` \| `turbo` \| `manual` | `ok` / `error` | Désactive l'auto TDP. Un mode firmware remet `tdp_watts` à `null`. Le firmware abandonne les courbes de ventilateurs personnalisées à chaque écriture du mode : le service les réécrit (une courbe qu'il ne peut pas réécrire est oubliée). Refusé pendant un banc d'essai. |
| `set_tdp` | `watts: u32` | `ok` / `error` | TDP manuel (sPPT, SPL, fPPT), mode `manual`, auto TDP désactivé. Refusé hors de la plage du modèle et pendant un banc d'essai. |
| `set_auto_tdp` | `settings: AutoTdpSettings` | `ok` / `error` | Active (ou désactive avec `enabled: false`) l'auto TDP. `min_watts`, `max_watts` et `battery_max_watts` sont bornés à la plage du modèle ; le statut et le profil gardent les valeurs bornées. Pendant un profil, modifie le profil. Refusé pendant un banc d'essai. |
| `set_battery_charge_limit` | `percent: u8` (40 à 100) | `ok` / `error` | Limite de charge de la batterie. Enregistrée et réappliquée au démarrage ; visible dans `status.battery_charge_limit`. Globale, pas par profil. **Non testé sur l'appareil.** |
| `foreground_app` | `exe: string \| null` | `ok` | Programme au premier plan (envoyé par le shell) ; applique ou quitte le profil correspondant. |
| `save_profile` | `profile: Profile` | `ok` / `error` | Crée ou remplace le profil de `profile.exe` (casse ignorée) ; le réapplique s'il est actif. |
| `delete_profile` | `exe: string` | `ok` | Supprime le profil ; rend la main à l'état général s'il était actif. |
| `list_profiles` | — | `profiles` | Tous les profils. |
| `start_bench` | `plan: BenchPlan` | `ok` / `error` | Lance un banc d'essai (il faut au moins 5 FPS). Le plan est borné à la plage du modèle ; pas et durées au moins à 1. |
| `cancel_bench` | — | `ok` / `error` | Arrête le banc d'essai en gardant un rapport partiel. |
| `get_bench_report` | — | `bench` | Le dernier rapport terminé ou annulé (pendant un banc d'essai, le précédent). |
| `set_accent` | `color: string` (`#rrggbb`) | `ok` / `error` | Couleur d'accent de l'interface, partagée par le shell et le widget. Validée (`#` et six chiffres hexadécimaux, sinon une erreur), stockée en minuscules, enregistrée dans `state.json` et poussée dans `status.accent`. |
| `set_fan_curve` | `fan` : `cpu` \| `gpu` \| `mid`, `curve: FanCurve` | `ok` / `error` | Courbe personnalisée d'un ventilateur, [validée](#courbes-de-ventilateurs) (plancher de sécurité compris). Conservée aux changements de mode, enregistrée dans `state.json`. Pendant un profil qui a ses propres courbes, modifie le profil ; sinon les courbes globales. |
| `get_fan_curves` | — | `fan_curves` | Les courbes personnalisées et celles du firmware pour le mode courant (point de départ d'un éditeur). |
| `set_fan_preset` | `preset: "balanced" \| "max_cooling" \| null` | `ok` / `error` | Le préréglage sur les ventilateurs CPU et GPU, `null` pour les courbes du firmware (comme `reset_fan_curves`). Le statut le renvoie dans `fan_preset` (`null` sans courbe ou avec d'autres courbes). |
| `save_foreground_profile` | `part: "power" \| "hardware"` | `ok` / `error` | Enregistre dans le profil du jeu au premier plan (créé avec l'auto TDP actuel s'il n'existe pas) l'auto TDP et la puissance tenue (`power`), ou les courbes et la limite de FPS (`hardware`) ; le reste du profil est gardé. Erreur sans jeu au premier plan. Utilisé par le widget. |
| `set_fps_limit` | `fps: u32 \| null` (30 à 120) | `ok` / `error` | Plafond de FPS du pilote AMD (secteur et batterie, tous les jeux) ; `null` le retire. Enregistré ; pendant un profil qui a son propre plafond, modifie le profil. **Non testé depuis la session 0 du service.** |
| `set_controller_mode` | `mode` : `gamepad` \| `wasd` \| `mouse` | `ok` / `error` | Mode de la manette intégrée (HID). Enregistré et réappliqué au démarrage. Global, pas par profil. **Non testé sur l'appareil.** |
| `set_gyro` | `settings: GyroSettings` | `ok` / `error` | Visée au gyroscope. Seulement stockée : le shell l'applique. Enregistrée ; pendant un profil qui a ses propres réglages de gyroscope, modifie le profil. |
| `set_back_button` | `button` : `m1` \| `m2`, `target: ButtonTarget` | `ok` / `error` | Ce qu'envoie un bouton arrière, écrit dans la manette (HID). Enregistré, réappliqué au démarrage et après chaque `set_controller_mode`. Global. **Non testé sur l'appareil.** |
| `reset_back_buttons` | — | `ok` / `error` | Les deux boutons arrière reviennent à leurs propres touches (l'association d'usine est écrite). |
| `set_lighting` | `lighting: Lighting \| null` | `ok` / `error` | Éclairage des anneaux des sticks (HID). Enregistré et réécrit au démarrage ; sans couleur, les anneaux prennent l'accent (`set_accent` les réécrit, `#ff3d5e` avant tout accent). `null` : Canapé cesse de les écrire, rien n'est envoyé (l'éclairage propre de la manette devrait revenir au prochain redémarrage). **Non testé sur l'appareil.** |

Exemples :

```json
{"type":"set_performance_mode","mode":"turbo"}
{"type":"set_auto_tdp","settings":{"enabled":true,"target_fps":60,"min_watts":7,"max_watts":25,"battery_max_watts":15,"max_temp_c":85}}
{"type":"set_battery_charge_limit","percent":80}
{"type":"foreground_app","exe":"eldenring.exe"}
{"type":"foreground_app","exe":null}
{"type":"save_profile","profile":{"exe":"eldenring.exe","auto_tdp":{"enabled":true,"target_fps":60,"min_watts":7,"max_watts":19,"battery_max_watts":null,"max_temp_c":null},"learned_watts":16}}
{"type":"delete_profile","exe":"eldenring.exe"}
{"type":"start_bench","plan":{"from_watts":25,"to_watts":7,"step_watts":3,"settle_secs":5,"measure_secs":15}}
{"type":"set_accent","color":"#14b8a6"}
{"type":"set_fan_curve","fan":"cpu","curve":[[40,10],[50,18],[58,26],[65,34],[72,42],[78,50],[85,62],[92,75]]}
{"type":"reset_fan_curves"}
{"type":"get_fan_curves"}
{"type":"set_fps_limit","fps":40}
{"type":"set_fps_limit","fps":null}
{"type":"set_controller_mode","mode":"mouse"}
{"type":"set_gyro","settings":{"activation":"hold","sensitivity":10,"horizontal":"yaw","invert_y":false}}
```

### Réponses

| Réponse | Champs | Exemple |
|---|---|---|
| `hello` | `protocol_version` | `{"type":"hello","protocol_version":1}` |
| `status` | les champs du [statut](#le-statut), à plat | `{"type":"status","tdp_watts":16,…}` |
| `ok` | — | `{"type":"ok"}` |
| `error` | `message` (anglais, non traduit) | `{"type":"error","message":"a bench is running: cancel it first"}` |
| `profiles` | `profiles: [Profile]` | `{"type":"profiles","profiles":[…]}` |
| `bench` | `report: BenchReport \| null` | `{"type":"bench","report":null}` |
| `fan_curves` | `custom: FanCurves`, `defaults: FanCurves` | `{"type":"fan_curves","custom":{"cpu":null,"gpu":null,"mid":null},"defaults":{"cpu":[[58,8],[61,17],…],"gpu":[…],"mid":null}}` |

### Le statut

```json
{
  "type": "status",
  "tdp_watts": 16,
  "performance_mode": "manual",
  "auto_tdp": {"enabled": true, "target_fps": 60, "min_watts": 7, "max_watts": 25, "battery_max_watts": 15, "max_temp_c": 85},
  "fps": 61.2,
  "gpu_util": 0.93,
  "power_watts": 14.6,
  "cpu_temp_c": 78.0,
  "battery_percent": 64,
  "conflicts": [],
  "foreground": "eldenring.exe",
  "active_profile": "eldenring.exe",
  "on_battery": true,
  "thermal_limited": false,
  "bench": null,
  "accent": "#ff3d5e",
  "tdp_min_watts": 5,
  "tdp_max_watts": 30,
  "cpu_fan_rpm": 3400,
  "gpu_fan_rpm": 3100,
  "fan_curves": {"cpu": [[40,10],[50,18],[58,26],[65,34],[72,42],[78,50],[85,62],[92,75]], "gpu": null, "mid": null},
  "fps_limit": 40,
  "controller_mode": "gamepad",
  "gyro": {"activation": "hold", "sensitivity": 10, "horizontal": "yaw", "invert_y": false},
  "back_buttons": {"m1": "default", "m2": "pad_a"},
  "lighting": {"level": 2, "color": null}
}
```

| Champ | Type | Signification |
|---|---|---|
| `tdp_watts` | `u32 \| null` | Limite appliquée par Canapé. `null` en mode firmware (le firmware ne la communique pas) ou avant tout réglage. |
| `performance_mode` | `string \| null` | `silent`, `performance`, `turbo`, `manual`, ou `null` si rien n'a encore été appliqué. |
| `auto_tdp` | `AutoTdpSettings` | Réglages en vigueur : globaux, ou ceux du profil actif. Puissances bornées à la plage du modèle à l'application. |
| `fps` | `f32 \| null` | FPS du dernier tick (pilote AMD). |
| `gpu_util` | `f32 \| null` | Activité GPU entre 0 et 1. |
| `power_watts` | `f32 \| null` | Ce que l'APU consomme réellement, en watts (`tdp_watts` n'en est que la limite). Tracé face à la limite dans les graphes du shell et du widget. Facultatif en entrée. |
| `cpu_temp_c` | `f32 \| null` | Température CPU en °C. |
| `battery_percent` | `u8 \| null` | Charge de la batterie. |
| `foreground` | `string \| null` | Programme au premier plan signalé par le shell. |
| `active_profile` | `string \| null` | `exe` du profil appliqué. |
| `on_battery` | `bool \| null` | `true` sur batterie, `false` sur secteur. |
| `thermal_limited` | `bool` | L'auto TDP retient la puissance à cause de la limite de température. |
| `bench` | `BenchProgress \| null` | Progression du banc d'essai en cours. |
| `accent` | `string \| null` | Couleur d'accent `#rrggbb`, dès qu'un client en a choisi une (`set_accent`). Le shell l'adopte quand elle change ailleurs et donne la sienne à un service qui n'en a pas ; le widget la suit. Facultatif en entrée. |
| `tdp_min_watts`, `tdp_max_watts` | `u32 \| null` | Plage de puissance soutenue du modèle (5–30 W sur RC71L / RC72LA, 5–35 W sur RC73 ; 7–30 W dans le simulateur) : les bornes d'un sélecteur de TDP. `null` avec un service plus ancien. |
| `cpu_fan_rpm`, `gpu_fan_rpm` | `u32 \| null` | Vitesse des ventilateurs, par pas de 100 tr/min ; `null` quand le firmware ne donne pas de lecture valable. |
| `fan_curves` | `FanCurves` | Courbes personnalisées en vigueur (globales ou du profil actif) ; un ventilateur à `null` suit la courbe du mode. |
| `fps_limit` | `u32 \| null` | Plafond de FPS posé par Canapé (`null` : aucun posé par Canapé ; le logiciel AMD peut en avoir un). |
| `controller_mode` | `string \| null` | `gamepad`, `wasd`, `mouse`, ou `null` si Canapé ne l'a pas réglé. |
| `battery_charge_limit` | `u8 \| null` | Limite de charge en pour cent, ou `null` si Canapé ne l'a pas réglée (le firmware ne la donne pas). Facultatif en entrée. |
| `gyro` | `GyroSettings` | Visée au gyroscope en vigueur : globale, ou celle du profil actif. Valeurs par défaut si absent. |
| `back_buttons` | `BackButtons` | Ce qu'envoient M1 et M2 ; `default` pour les deux tant que Canapé ne les a pas changés. Facultatif en entrée. |
| `lighting` | `Lighting \| null` | Anneaux des sticks réglés par Canapé, `null` quand ils sont laissés à la manette. Facultatif en entrée. |

### Types imbriqués

**`AutoTdpSettings`**

| Champ | Type | Défaut | |
|---|---|---|---|
| `enabled` | `bool` | `false` | |
| `target_fps` | `u32` | `60` | 0 refusé |
| `min_watts` | `u32` | `7` | |
| `max_watts` | `u32` | `25` | |
| `battery_max_watts` | `u32 \| null` | `null` | facultatif en entrée |
| `max_temp_c` | `u32 \| null` | `null` | facultatif en entrée |

**`Profile`** : `exe` (string, comparé sans la casse), `auto_tdp` (`AutoTdpSettings`), `learned_watts` (`u32 | null`, facultatif), `fan_curves` (`FanCurves | null`, facultatif : `null` garde les courbes globales), `fps_limit` (`u32 | null`, facultatif : `null` garde le plafond global ; un profil ne peut pas imposer « aucun plafond » quand un plafond global existe), `refresh_hz` (`u32 | null`, facultatif, appliqué par le shell), `gyro` (`GyroSettings | null`, facultatif : `null` garde les réglages globaux).

```json
{"exe":"eldenring.exe","auto_tdp":{"enabled":true,"target_fps":40,"min_watts":7,"max_watts":20},"learned_watts":14,"fan_curves":{"cpu":[[30,35],[40,45],[50,55],[60,65],[70,80],[75,90],[80,100],[85,100]]},"fps_limit":40}
```

**`FanCurve`** : huit paires `[température °C, rapport cyclique %]`, pour le ventilateur CPU comme pour le GPU. Les règles (`FanCurve::validate` dans `canape-core/src/fan.rs`, vérifiées aussi par le backend Ally avant l'écriture) :

- rapports de 0 à 100 %, jamais décroissants ;
- **plancher de sécurité** : le rapport du dernier point à 80 °C ou en dessous doit valoir au moins 25 %, à 90 °C au moins 30 %, à 100 °C au moins 40 % ; s'il n'y a aucun point à ou sous l'une de ces températures, il compte pour 0 %. Cette borne basse tient que le firmware interpole ou procède par paliers entre les points. Les valeurs sont juste sous les courbes Silencieux d'ASUS (28 / 34 / 41 % pour le ventilateur CPU) : une courbe peut être aussi discrète que le mode Silencieux d'usine, jamais plus quand il fait chaud. 0 % à 90 °C est donc impossible.

**`FanCurves`** : `{"cpu": FanCurve | null, "gpu": FanCurve | null, "mid": FanCurve | null}`, chaque champ facultatif. `mid` n'existe que sur certains portables : l'Ally le refuse.

**Préréglages** (`FanPreset` dans `canape-core`, des données pour les interfaces, valides par construction) :

| Préréglage | Points |
|---|---|
| `silent` | `[[30,0],[45,0],[55,10],[62,15],[68,20],[75,26],[85,34],[95,45]]` |
| `balanced` | `[[40,10],[50,18],[58,26],[65,34],[72,42],[78,50],[85,62],[92,75]]` |
| `max_cooling` | `[[30,35],[40,45],[50,55],[60,65],[70,80],[75,90],[80,100],[85,100]]` |

**`ControllerMode`** : `gamepad` (XInput), `wasd` (clavier), `mouse` (bureau).

**`GyroSettings`** : `activation` (`off`, `hold` : tant qu'une palette arrière est tenue, `toggle` : un appui sur une palette active et coupe, `always` ; `off` par défaut), `sensitivity` (comptes de souris par degré, 1 à 50, 10 par défaut), `horizontal` (`yaw` : tourner autour de l'axe vertical de l'écran, ou `roll` : incliner comme un volant ; `yaw` par défaut), `invert_y` (`false` par défaut). Tous les champs facultatifs en entrée.

**`BackButtons`** : `{"m1": ButtonTarget, "m2": ButtonTarget}`, chacun `default` s'il manque.

**`Lighting`** : `level` 0 (éteint) à 3, `color` `#rrggbb` ou `null` (suivre l'accent). Un niveau au-dessus de 3 ou une couleur mal formée est une erreur ; la couleur est gardée en minuscules.

**`ButtonTarget`** : `default` (la propre touche du bouton), `disabled` ; manette `pad_a`, `pad_b`, `pad_x`, `pad_y`, `pad_lb`, `pad_rb`, `pad_lt`, `pad_rt`, `pad_ls`, `pad_rs` (clics des sticks), `dpad_up`, `dpad_down`, `dpad_left`, `dpad_right`, `pad_view`, `pad_menu`, `pad_xbox` ; clavier `key_escape`, `key_enter`, `key_space`, `key_tab`, `key_backspace`, `key_up`, `key_down`, `key_left`, `key_right`, `key_page_up`, `key_page_down`, `key_shift`, `key_ctrl`, `key_windows`, `key_f11`, `key_f12` ; souris `mouse_left`, `mouse_right`, `mouse_middle`, `wheel_up`, `wheel_down` ; média `volume_up`, `volume_down`, `mute_mic`, `show_keyboard` (le clavier tactile). `ButtonTarget::ALL` garde cet ordre.

**`BenchPlan`** : `from_watts` (25), `to_watts` (7), `step_watts` (3), `settle_secs` (5), `measure_secs` (15).

**`BenchProgress`** : `step` (à partir de 1), `steps`, `watts`, `seconds_left`. Exemple : `{"step":2,"steps":7,"watts":22,"seconds_left":110}`.

**`BenchReport`**

```json
{
  "exe": "eldenring.exe",
  "started_unix": 1792615847,
  "plan": {"from_watts": 25, "to_watts": 7, "step_watts": 3, "settle_secs": 5, "measure_secs": 15},
  "points": [
    {"watts": 7, "fps_avg": 31.0, "fps_low": 25.0, "gpu_avg": 0.98, "temp_max": 67.0, "samples": 15},
    {"watts": 10, "fps_avg": 44.0, "fps_low": 38.0, "gpu_avg": 0.97, "temp_max": 70.0, "samples": 15}
  ],
  "completed": true
}
```

`points` est trié par puissance croissante. Les recommandations (`sweet_spot`, `watts_for`, `most_efficient`) sont des méthodes de `BenchReport`, calculées côté client ; voir [Auto TDP et banc d'essai](auto-tdp.fr.md#les-trois-recommandations).

### Erreurs

| Message | Cause |
|---|---|
| `unsupported protocol version N, expected 1` | `hello` avec une autre version |
| `a bench is running: cancel it first` | `set_performance_mode`, `set_tdp`, `set_auto_tdp` ou `start_bench` pendant un banc d'essai |
| `invalid auto TDP settings` | Minimum au-dessus du maximum après bornage au modèle, ou objectif à 0 |
| `a profile needs an executable name` | `save_profile` avec un `exe` vide |
| `no bench is running` | `cancel_bench` sans banc d'essai |
| `no frame rate: start a game before the bench` | `start_bench` sous 5 FPS |
| `not a #RRGGBB colour: …` | `set_accent` avec autre chose que `#` et six chiffres hexadécimaux |
| `fan curve: …` | `set_fan_curve` qui enfreint une [règle](#types-imbriqués) (ordre, plage, rapport décroissant, plancher de sécurité) |
| `frame rate limit not in 30..=120 FPS` | `set_fps_limit` hors plage |
| `gyro sensitivity not in 1..=50` | `set_gyro` hors plage |
| `not supported on this device: …` | Pas de ventilateur central, pas de pilote AMD, pas d'interface HID de la manette, ou fonction absente du simulateur |
| `GPU driver refused: ADL2_FPS_Settings_Set(…) on adapter … returned …` | Le pilote AMD a refusé le plafond (code dans le message) |
| `fan curves reset, but the TDP was not applied again: …` | `reset_fan_curves` en mode manuel, échec de la réécriture du TDP |
| `value out of range: …` | Watts hors de la plage du modèle, limite de charge hors de 40 à 100 %, courbe invalide arrivée jusqu'au backend |
| `firmware refused: DEVS 0x… = … returned …` | Le firmware n'a pas renvoyé 1 (pour une courbe, le mode est alors réécrit pour retrouver les courbes d'usine) |
| `device I/O failed: …` | `DeviceIoControl` a échoué |

### Faire évoluer le protocole

- Un nouveau champ de `Status`, `AutoTdpSettings` ou `Profile` prend `#[serde(default)]` : les anciens clients l'ignorent, et un ancien `state.json` reste lisible. `Profile` dérive `Default` : les clients Rust le construisent avec `..Profile::default()` pour qu'un nouveau champ ne les casse pas.
- Une nouvelle requête est une nouvelle variante de `Request` ; pensez à l'ajouter à la liste des requêtes en lecture seule de `server.rs` si elle ne modifie rien (sinon elle est journalisée et déclenche un enregistrement).
- Un changement incompatible augmente `PROTOCOL_VERSION`, côté Rust **et** dans `CanapeLink.cs`.
- Le widget ne réécrit que les champs qu'il connaît : `set_auto_tdp` renvoie l'objet `auto_tdp` reçu en entier, en ne changeant que `enabled` et `target_fps`.

## Compiler et tester sous Linux

La session cloud n'a pas de matériel : tout se teste avec le simulateur (`MockDevice`).

**Dépendances** (Debian / Ubuntu, comme la CI) :

```sh
sudo apt-get install -y libxkbcommon-dev libfontconfig1-dev
```

Pour afficher le shell, il faut un serveur X (le backend Slint est winit + X11, rendu logiciel). Le shell active la feature Slint `software-renderer-path` à côté de `renderer-software` : sans elle, le rendu logiciel ne dessine pas les éléments `Path` et les graphes restent vides.

**Vérifications avant de pousser** (règle du projet) :

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
rustup target add x86_64-pc-windows-msvc   # une fois
cargo check --target x86_64-pc-windows-msvc --all-targets
```

`cargo test` lance plus de 90 tests à ce jour : contrôleur d'auto TDP (dont une simulation du cas d'oscillation de M4), handler du service (profils, banc d'essai, persistance), banc d'essai, tampons ATKACPI, règles et préréglages des courbes de ventilateurs, rapports de la manette, SDDL du pipe et hachage de l'éditeur, IPC, navigation à la manette, suivi du premier plan, langues et complétude des catalogues. Le `cargo check` Windows attrape le code `#[cfg(windows)]` que Linux ne compile pas, sans pouvoir l'exécuter.

### Service et shell avec le simulateur

```sh
# Terminal 1 : le service, avec un « jeu » simulé à 60 FPS
CANAPE_MOCK_FPS=60 cargo run -p canape-service -- --console

# Terminal 2 : suivre le statut, envoyer des commandes
cargo run -p canape-ctl -- watch
cargo run -p canape-ctl -- '{"type":"set_auto_tdp","settings":{"enabled":true,"target_fps":60,"min_watts":7,"max_watts":25}}'
cargo run -p canape-ctl -- --pretty '{"type":"get_fan_curves"}'

# Terminal 3 : le shell dans une fenêtre
CANAPE_WINDOWED=1 cargo run -p canape-shell
```

Le simulateur : plage de 7 à 30 W, activité GPU 70 %, 55 °C, batterie 80 % sur secteur, limite de charge 40 à 100 %. Ventilateurs : CPU et GPU (pas de central), courbes par mode tirées des préréglages (silencieux → `silent`, turbo → `max_cooling`, sinon `balanced`), une écriture du mode efface les courbes personnalisées comme sur l'Ally, vitesse = rapport × 6000 tr/min. Un plafond de FPS plafonne les FPS simulés. Les FPS restent ceux de `CANAPE_MOCK_FPS` quelle que soit la puissance : l'auto TDP descend donc jusqu'au minimum (un objectif à 60 avec 60 FPS et 70 % de GPU déclenche la règle du GPU au repos).

Au clavier, le shell se pilote comme à la manette : Tab / Maj+Tab, Entrée ou Espace, Échap, Page préc. / Page suiv.

Sous Linux, le shell ne suit pas le premier plan. Pour simuler un jeu, envoyez-le au service :

```sh
cargo run -p canape-ctl -- '{"type":"foreground_app","exe":"eldenring.exe"}'
cargo run -p canape-ctl -- '{"type":"start_bench","plan":{"from_watts":25,"to_watts":7,"step_watts":3,"settle_secs":1,"measure_secs":2}}'
```

Hors Windows, les journaux et l'état vont dans `<tmp>/Canape/` (`service.log`, `state.json`, `shell.log`, `shell.json`, avec `<tmp>` = `$TMPDIR` ou `/tmp`). Supprimez `state.json` pour repartir d'un état vierge.

### Variables d'environnement

| Variable | Lue par | Effet |
|---|---|---|
| `CANAPE_MOCK_FPS` | service (simulateur) | FPS simulés, par exemple `60`. Sans elle, `fps` vaut `null` et l'auto TDP ne bouge pas. Sert aussi sous Windows quand le simulateur est utilisé. |
| `CANAPE_SOCKET` | service, shell, `canape-ctl` (hors Windows) | Chemin du socket Unix. |
| `CANAPE_WINDOWED` | shell | Présente (n'importe quelle valeur) : fenêtre au lieu du plein écran. |
| `CANAPE_PAGE` | shell | Page ouverte au démarrage : 0 Jeux, 1 Performance, 2 Matériel, 3 Réglages, 4 Profils de jeu, 5 Banc d'essai, 6 Visée au gyroscope, 7 Boutons arrière et éclairage, 8 Ajouter un jeu. |
| `CANAPE_MOCK_GYRO` | shell (`canape_hw::motion::open`) | `x,y,z` en degrés par seconde : un gyroscope simulé qui tourne à cette vitesse, à la place du capteur Windows. |
| `CANAPE_LIBRARY_FIXTURE` | shell (builds de debug) | Une fausse bibliothèque à la place des boutiques installées : voir [La bibliothèque de jeux](#la-bibliothèque-de-jeux). |
| `LC_ALL`, `LC_MESSAGES`, `LANGUAGE`, `LANG` | shell (hors Windows) | Langue en mode Automatique, dans cet ordre de priorité. |
| `TMPDIR` | tous (hors Windows) | Dossier temporaire, donc emplacement de `Canape/`. |

### Captures d'écran avec Xvfb

Les captures de `docs/images/` sont faites ainsi (recette vérifiée) :

```sh
export TMPDIR=$(mktemp -d) CANAPE_SOCKET=/tmp/doc.sock   # state.json et shell.json vierges
Xvfb :99 -screen 0 1280x720x24 &
CANAPE_MOCK_FPS=60 ./target/debug/canape-service --console &
./target/debug/canape-ctl '{"type":"foreground_app","exe":"eldenring.exe"}'   # plus profils, accent, banc d'essai court…
DISPLAY=:99 CANAPE_WINDOWED=1 CANAPE_PAGE=1 LANG=fr_FR.UTF-8 ./target/debug/canape-shell &
for w in 15 18 22 17 12 9 13 19 …; do ./target/debug/canape-ctl "{\"type\":\"set_tdp\",\"watts\":$w}"; sleep 2.5; done
DISPLAY=:99 import -window root docs/images/fr/performance.png   # ImageMagick
```

- 1280×720 correspond à l'écran 1080p de l'Ally à 150 %.
- `LANG=<locale>` (ou `LANGUAGE=<code>`) choisit la langue (le mode Automatique la suit) ; sinon, enregistrez un choix dans `<tmp>/Canape/shell.json` (`{"language":"ja"}`).
- Préparez l'état avec `canape-ctl` (profils, premier plan, accent, banc d'essai) avant de lancer le shell. Un banc d'essai court (`settle_secs` 1, `measure_secs` 2) remplit la page de résultats en 20 s environ.
- Les graphes gardent la dernière minute **dans le shell** : laissez-le tourner au moins 60 s pendant que le TDP change (le simulateur consomme 90 % de la limite, avec un GPU fixe à 70 %), sinon les courbes sont plates ou courtes.
- Le shell n'est à instance unique que sous Windows : sous Linux, un écran Xvfb par shell permet de capturer plusieurs pages à la fois.
- Fichiers : `docs/images/<langue>/<page>.png` (`performance`, `hardware`, `profiles`, `bench-results`, `bench-running`, `settings`, `library`, `game-page`, `library-install` et `add-game` avec `tools/fake-library.py`, qui écrit aussi l'exécutable appris d'Ashen Tide pour que sa fiche montre un profil), `docs/images/accent-<couleur>.png` pour les variantes d'accent. Optimisez avec `pngquant` puis `optipng`.
- Un `shell.json` ou un `state.json` resté dans `<tmp>/Canape/` change ce qui s'affiche : utilisez un `TMPDIR` dédié pour des captures reproductibles.

## Compiler et tester sous Windows

**Prérequis** : Rust (toolchain MSVC), Visual Studio Build Tools 2022 avec les outils C++ et le Windows SDK, mode développeur.

```powershell
cargo build --release -p canape-shell -p canape-service -p canape-ctl
```

> [!WARNING]
> **Smart App Control** bloque les build scripts non signés d'une compilation neuve (`os error 4551`). Solutions : relancer, compiler en debug, utiliser les binaires de la CI, ou désactiver Smart App Control (difficile à réactiver). Voir [m1-report.md](m1-report.md) et [m2-report.md](m2-report.md).

**Installer** sur la machine : `packaging\install-dev.ps1` (voir le [guide d'utilisation](user-guide.fr.md#installer-canapé)).

**Lancer le service à la main** pour voir son journal :

```powershell
Stop-Service CanapeService       # un seul serveur peut posséder le pipe
& .\target\release\canape-service.exe --console
```

> [!IMPORTANT]
> Lancé **sans droits administrateur**, ou sur un PC qui n'est pas une Ally reconnue, le service ne peut pas ouvrir `\\.\ATKACPI` et **bascule silencieusement sur le simulateur** (`device = mock` dans le journal). Vérifiez toujours la ligne `device = ROG Ally`.

**Pièges de Windows PowerShell 5.1** :

- les guillemets d'un argument JSON passé à un `.exe` doivent être échappés (`'{\"type\":\"get_status\"}'`) ;
- `if (…) { @("--release") }` renvoie une chaîne, que le splatting passe caractère par caractère (corrigé dans `install-dev.ps1`) ;
- `Stop-Process` rend la main avant que l'exécutable soit libéré : attendre avec `Wait-Process`.

Pour tester le shell **avec l'identité du paquet** (comme le FSE), passez par `install-dev.ps1` puis `explorer.exe shell:AppsFolder\Canape_<hash>!App`.

## Intégration continue

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) tourne à chaque push, pull request ou lancement manuel :

| Job | Étapes |
|---|---|
| `windows` (windows-latest) | `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release` des trois binaires, artefact **`canape-windows-x64`** (`canape-shell.exe`, `canape-service.exe`, `canape-ctl.exe`) |
| `linux` (ubuntu-latest) | Dépendances X11 / fontconfig, `cargo clippy --all-targets -- -D warnings`, `cargo test` |

L'artefact s'installe sur l'Ally avec `install-dev.ps1 -BinariesDir` : c'est le moyen de contourner Smart App Control. Le widget n'est pas compilé par la CI.

Profil release (`Cargo.toml`) : `opt-level = "s"`, LTO, `codegen-units = 1`, `panic = "abort"`, symboles retirés.

## Empaquetage


| Fichier | Rôle |
|---|---|
| [`packaging/identity/AppxManifest.xml`](../packaging/identity/AppxManifest.xml) | Identité `Canape`, éditeur `CN=Canape Dev`, Windows 10.0.26100 minimum. `uap10:AllowExternalContent`. Application `App` → `canape-shell.exe`, `uap10:TrustLevel="mediumIL"`, `uap10:RuntimeBehavior="win32App"`. Extension `uap3:AppExtension Name="windows.gamingApp"`. Capacités `runFullTrust`, `unvirtualizedResources` et `Microsoft.appCategory.gamingHome_8wekyb3d8bbwe`. |
| [`packaging/identity/CustomCapability.SCCD`](../packaging/identity/CustomCapability.SCCD) | Débloque la capacité `gamingHome`. `AllowAny="true"`, catalogue `FFFF` (test) : **mode développeur obligatoire**. Copié aussi à la racine de l'installation. |
| [`crates/canape-shell/canape-shell.exe.manifest`](../crates/canape-shell/canape-shell.exe.manifest) | Intégré à l'exe par `build.rs` (MSVC) : élément `msix` qui lie l'exe au paquet (`publisher`, `packageName`, `applicationId` identiques au manifeste du paquet), `asInvoker`, DPI PerMonitorV2. |
| [`packaging/install-dev.ps1`](../packaging/install-dev.ps1) | Compile, installe le service, crée le certificat, `makeappx pack`, `signtool sign`, `Add-AppxPackage -ExternalLocation`, et éventuellement `GamingHomeApp`. |
| [`packaging/uninstall-dev.ps1`](../packaging/uninstall-dev.ps1) | L'inverse, widget compris : par `canape-ctl`, `reset_fan_curves`, `set_fps_limit` `null`, `set_controller_mode` `gamepad` et, si `status.battery_charge_limit` est sous 100, `set_battery_charge_limit` 100 ; puis `canape-service uninstall` (`sc.exe delete` si l'exe a disparu), `Remove-AppxPackage` de `Canape` et `Canape.Widget`, `GamingHomeApp` effacée quand elle commence par le nom de famille du paquet, dossier d'installation supprimé. `-RemoveData` (`%ProgramData%\Canape`, `%LOCALAPPDATA%\Canape`), `-RemoveCertificate`. **Pas encore essayé sur l'Ally.** |

**Certificat de développement** : auto-signé, sujet `CN=Canape Dev`, usage « signature de code » (`1.3.6.1.5.5.7.3.3`), créé dans `CurrentUser\My`, exporté en `.pfx` (mot de passe `canape-dev`) et `.cer` dans `$HOME\dev\canape-cert`, le `.cer` importé dans `LocalMachine\TrustedPeople`. Il signe le paquet d'identité et le widget. L'éditeur du certificat, du manifeste du paquet et du manifeste de l'exe doivent rester identiques.

**Choix de l'accueil** : par l'utilisateur dans Paramètres → Jeux → Expérience plein écran, ou en écrivant la valeur non documentée `HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration\GamingHomeApp = <PFN>!App`. Au boot, si `StartupToGamingHome = 1`, Windows active cet AUMID avec l'argument `windows.gaming:///home` (`args=[]` hors FSE).

> [!NOTE]
> Une distribution hors mode développeur demanderait un SCCD signé par Microsoft. C'est hors périmètre tant que Canapé reste un projet personnel.

## Le widget Game Bar

| Point | Choix |
|---|---|
| Technologie | **C# UWP compilé en .NET Native** : la Game Bar n'accepte que des apps UWP XAML (pas WinUI 3). Projet dérivé de l'exemple officiel `WidgetSampleCS` (MIT). |
| SDK | `Microsoft.Gaming.XboxGameBar` **7.2.240903001** (la 7.3.x casse la compilation). Le bloc `proxyStub` du manifeste correspond à cette version. |
| Cible | `TargetPlatformVersion` 10.0.26100.0 (SDK installé sur l'Ally), `TargetPlatformMinVersion` 10.0.19041.0. `mp:PhoneIdentity` requis par les cibles d'empaquetage UWP. |
| Pipe | `CreateFileFromAppW` (autorisé depuis un AppContainer depuis Windows 10 2004). Une connexion d'abonnement, puis une connexion courte par commande, reconnexion toutes les 2 s. |
| Paquet | `Canape.Widget`, extension `microsoft.gameBarUIExtension`, signé avec le certificat de développement. |
| Fichiers | `widget/CanapeWidget/` : `CanapeLink.cs` (client du pipe), `WidgetPage.xaml(.cs)` (interface), `Strings/<langue>/Resources.resw`, `Package.appxmanifest`. |

**Compiler** : workload `Microsoft.VisualStudio.Workload.UniversalBuildTools` puis `packaging\build-widget.ps1` (voir le [guide d'utilisation](user-guide.fr.md#le-widget-xbox-game-bar)). Le script lance MSBuild (Release x64, sideload, signé), puis `Add-AppxPackage` en ne passant **que les frameworks absents ou plus anciens** : réinstaller VCLibs ou .NET Native alors que la Game Bar les utilise échoue avec `0x80073D02` ([m3-report.md](m3-report.md)).

Le widget ne se compile que sur Windows avec les outils UWP : il n'est vérifié ni par la CI, ni par la session cloud.

## Traductions

### Shell (Slint + gettext)

- Toute chaîne visible passe par `@tr("…")` dans `crates/canape-shell/ui/*.slint`. Les nombres restent des nombres jusqu'à l'affichage : les formats (`"{} W"`, `"{} %"`, `"{} °C"`, le séparateur décimal) sont eux-mêmes traduits.
- Catalogues : `crates/canape-shell/lang/<code>/LC_MESSAGES/canape-shell.po`, intégrés à l'exe par `build.rs` (`with_bundled_translations("lang")`, sans contexte par défaut : une chaîne est traduite une seule fois). Seule exception : `msgctxt "decimal separator"` pour `"."`.
- L'anglais est la langue source : pas de catalogue.
- `shell.json` (`%LOCALAPPDATA%\Canape\`, `<tmp>/Canape/` hors Windows) contient `language` et `accent` (et, entre autres, `games_folder`, voir [Installer les jeux possédés](#installer-les-jeux-possédés)). `accent` est désormais une couleur hexadécimale `#rrggbb` ; les codes des versions précédentes (`red`, `blue`, `violet`, `green`, `orange`, `cyan`) sont convertis au chargement, toute autre valeur donne `#ff3d5e`. Un choix est enregistré et envoyé par `set_accent` après 600 ms sans changement.
- `i18n.rs` choisit le catalogue : choix enregistré dans `shell.json` s'il est fourni, sinon la première langue préférée de Windows (`GetUserPreferredUILanguages`) ou de l'environnement POSIX qui est fournie, sinon l'anglais. Correspondance par code de base (`pt-BR` → `pt`, `zh-*` → `zh_CN`).

**Ajouter une langue au shell**

1. Ajoutez le code (nom du dossier) à `LANGUAGES` dans [`i18n.rs`](../crates/canape-shell/src/i18n.rs). Si le code ne correspond pas au code de base des balises de Windows, adaptez `match_locale` (comme pour `zh_CN`).
2. Copiez `lang/fr/LC_MESSAGES/canape-shell.po` vers `lang/<code>/LC_MESSAGES/canape-shell.po`, mettez à jour l'en-tête (`Language`, `Plural-Forms`) et traduisez chaque `msgstr`. **Une ligne par champ** : le test ne lit pas les chaînes sur plusieurs lignes. Gardez le même nombre de `{}`.
3. Ajoutez une tuile dans la grille `rows` de `SettingsPage` ([`pages.slint`](../crates/canape-shell/ui/pages.slint)), avec le nom de la langue dans la langue elle-même (il reste deux cases vides).
4. `cargo test -p canape-shell` : le test `every_language_translates_every_string` vérifie que chaque catalogue contient chaque `@tr` des sources, sans traduction vide, avec les mêmes `{}`, et sans entrée périmée.
5. Faites des captures de chaque page avec Xvfb et `LANGUAGE=<code>` : les textes trop longs sont coupés (`…`).

Ajouter une chaîne à l'interface demande d'ajouter son entrée dans les 12 catalogues, sinon ce même test échoue.

### Widget (.resw)

- Chaque chaîne est dans `widget/CanapeWidget/Strings/<balise>/Resources.resw` : `x:Uid` dans le XAML (clés `Nom.Propriété`), `ResourceLoader` dans le code (clés sans point), `ms-resource:` dans le manifeste. Les formats (`FormatWatts`, `FormatPercent`, `FormatCelsius`) sont des ressources.
- Langue par défaut `en-US` ; Windows choisit selon les langues préférées.

**Ajouter une langue au widget** : copiez `Strings/en-US/Resources.resw` vers `Strings/<balise BCP-47>/`, traduisez chaque `<value>` en gardant les clés et les `{0}`, ajoutez `<PRIResource Include="Strings\<balise>\Resources.resw" />` dans `CanapeWidget.csproj`, recompilez. Le détail est dans [`widget/README.md`](../widget/README.md).

Il n'y a pas de test automatique pour le widget. Pour vérifier que toutes les langues ont les mêmes clés que l'anglais :

```sh
cd widget/CanapeWidget/Strings
for f in */Resources.resw; do
  diff <(grep -o 'data name="[^"]*"' en-US/Resources.resw | sort) \
       <(grep -o 'data name="[^"]*"' "$f" | sort) > /dev/null || echo "$f differs"
done
```

## Notes sur le matériel

> [!IMPORTANT]

### ATKACPI

Le firmware ASUS s'atteint par `\\.\ATKACPI` (ouverture en lecture / écriture, partagée), IOCTL `0x0022240C`. Tampon d'entrée : `[méthode u32][longueur des arguments u32][arguments u32…]`, little-endian ; sortie sur 16 octets.

| Méthode | Valeur | Usage |
|---|---|---|
| `DSTS` | `0x53545344` | Lecture. Résultat = valeur + bit de présence `0x00010000` ; 0 si l'ID n'existe pas. |
| `DEVS` | `0x53564544` | Écriture. Le firmware renvoie 1 en cas de succès. |

| ID | Nom | Usage dans Canapé |
|---|---|---|
| `0x001200A0` | sPPT (PL2, boost lent) | TDP, écrit en premier |
| `0x001200A3` | SPL (PL1, soutenu) | TDP, écrit en second |
| `0x001200C1` | fPPT (boost rapide) | TDP, écrit en dernier, seulement si `DSTS` le dit présent |
| `0x00120057` | Limite de charge | 40 à 100 % |
| `0x00120094` | Température CPU | Lecture, valeurs de 1 à 124 °C |
| `0x00110013` / `0x00110014` | Vitesse ventilateur CPU / GPU | Lecture, voir [Courbes de ventilateurs](#courbes-de-ventilateurs) |
| `0x00110031` | Vitesse ventilateur central | Lecture ; sert aussi de test de présence |
| `0x00110024` / `0x00110025` / `0x00110032` | Courbe CPU / GPU / centrale | Écriture et lecture sur 16 octets, voir [Courbes de ventilateurs](#courbes-de-ventilateurs) |


### Modèles

Le modèle est lu dans `HKLM\HARDWARE\DESCRIPTION\System\BIOS\SystemProductName` :

| Contient | Modèle | Plage de TDP |
|---|---|---|
| `RC71` | ROG Ally (Z1 / Z1 Extreme) | 5 à 30 W |
| `RC72` | ROG Ally X | 5 à 30 W |
| `RC73YA` | ROG Xbox Ally (Z2 A) | 5 à 35 W |
| `RC73XA` | ROG Xbox Ally X (Z2 Extreme) | 5 à 35 W |



### ADL depuis la session 0

`atiadlxx.dll` (livrée avec le pilote AMD) est chargée à l'exécution : sans pilote, pas de télémétrie, mais pas d'erreur.

| Appel | Service (session 0) | Session utilisateur |
|---|---|---|
| `ADL2_Main_Control_Create`, liste des adaptateurs | OK, mais aucun adaptateur `present` (pas d'écran) | OK |
| `ADL2_Adapter_FrameMetrics_Start(adaptateur, 0)` | **−1** | 0 |
| `ADL2_Adapter_FrameMetrics_Get` | FPS seulement **après** un `Start` réussi depuis la session utilisateur | OK |
| `ADL2_New_QueryPMLogData_Get`, capteur 19 (activité GFX) | OK | OK |

D'où la solution : au démarrage, le shell appelle `canape_hw::FrameMetrics::arm()` (un `FrameMetrics_Start`, ligne `adl: …` dans `shell.log`) ; le comptage est global au pilote et le service lit ensuite les FPS. L'adaptateur choisi est l'iGPU AMD intégré qui existe, en préférant un adaptateur `present`. Diagnostic détaillé : [m4-report.md](m4-report.md).

**Limite connue** (M5, partie 2) : un jeu en plein écran exclusif réduit, puis resté caché derrière le shell une quarantaine de secondes, n'est plus compté quand il revient (FPS absents, donc ni auto TDP ni banc d'essai) jusqu'au prochain démarrage du jeu. Relancer `FrameMetrics_Start`, depuis le shell ou depuis un processus à part, n'y change rien.

> [!WARNING]

### Courbes de ventilateurs


| Sujet | Fait |
|---|---|
| Ventilateur central | Seulement si `DSTS 0x00110031` répond (`IsMidFanSupported`). |


> [!CAUTION]
> Écrire une courbe retire les ventilateurs du contrôle d'ASUS. Le plancher de sécurité interdit une courbe muette quand il fait chaud, et l'APU se bride toujours seul à sa limite de température, mais une courbe trop discrète rend l'appareil brûlant en main. En cas d'erreur du firmware, le service réécrit le mode pour retrouver les courbes d'usine.

### Plafond de FPS (ADL)



### Manette (HID)

| Sujet | Fait |
|---|---|
| Initialisation | `0x5A` + `ASUS Tech.Inc.` en ASCII, une fois avant la première commande (`InitInput`). |
| Mode | `[0x5A, 0xD1, 0x01, 0x01, mode]`, `1` manette, `2` WASD, `3` souris, puis `HidD_SetFeature`. |
| Association des boutons | `[0x5A, 0xD1, 0x02, paire, 0x2C]` puis 44 octets : quatre associations de 11 octets, le premier bouton de la paire, son association secondaire (bouton gauche sous l'écran maintenu), le second bouton, son association secondaire. Paires : `1` croix haut / bas, `2` croix gauche / droite, `3` clics des sticks, `4` gâchettes hautes, `5` A / B, `6` X / Y, `7` View / Menu, `8` **M2 / M1**, `9` gâchettes. |
| Association | Premier octet, le groupe : `0x01` manette (code à l'octet 1), `0x02` clavier (octet 2), `0x03` souris (octet 4), `0x05` média (octet 3), `0x04` combinaison de touches (nombre à l'octet 5, puis les codes) ; que des zéros, rien. |
| Codes proposés | Manette : A `01`, B `02`, X `03`, Y `04`, LB `05`, RB `06`, LS `07`, RS `08`, croix haut `09`, bas `0A`, gauche `0B`, droite `0C`, LT `0D`, RT `0E`, View `11`, Menu `12`, Xbox `13`. Clavier : Échap `76`, Entrée `5A`, Espace `29`, Tab `0D`, Retour arrière `66`, flèches haut `98`, bas `99`, gauche `9A`, droite `9B`, Page précédente `96`, Page suivante `97`, Maj gauche `88`, Ctrl gauche `8C`, Windows `82`, F11 `78`, F12 `07`, M1 `8F`, M2 `8E`. Souris : gauche `01`, droit `02`, milieu `03`, molette haut `04`, molette bas `05`. Média : couper le micro `01`, volume − `02`, volume + `03`, clavier tactile `19`. |
| M1 / M2 d'usine | `[02 00 8E …] [02 00 8E …] [02 00 8F …] [02 00 8F …]` : M2 envoie la touche `8E` et M1 la touche `8F`, y compris avec le bouton gauche sous l'écran maintenu, en mode manette comme en mode clavier. « Par défaut » et **Rétablir les boutons d'origine** écrivent exactement ceci. |
| Séquence | Le rapport d'initialisation la première fois, puis le rapport de la paire 8 seul : à chaque changement, au démarrage du service et après chaque écriture du mode (un changement de mode peut ramener les associations propres au mode). Les associations secondaires gardent toujours les touches d'usine. Rien n'est écrit après. |

**Non testé sur l'appareil.** D'où viennent ces faits (tous sous GPL ou LGPL, réimplémentés, pas copiés) :


**Éclairage des sticks**, sur la même interface et le même rapport :

| Sujet | Fait |
|---|---|
| Luminosité | `[0x5A, 0xBA, 0xC5, 0xC4, niveau]`, niveau 0 (éteint) à 3. |
| Couleur | `[0x5A, 0xD1, 0x08, 0x0C]` puis rouge, vert, bleu pour chacune de quatre zones (deux par anneau) : Canapé écrit la même couleur dans les quatre. |
| Séquence | Le rapport d'initialisation la première fois, la luminosité, puis la couleur (une écriture de luminosité peut ramener les couleurs propres de la manette). Éteint : luminosité 0, puis noir. Au démarrage, et de nouveau quand l'accent change pour des anneaux qui le suivent. |
| Par défaut | `set_lighting` `null` n'écrit rien : la couleur posée directement ne devrait durer que jusqu'au redémarrage de la manette, son éclairage propre devrait donc revenir alors. Non vérifié. |

**Non testé sur l'appareil.** Sources :


### Gyroscope

| Sujet | Fait |
|---|---|
| Capteur | IMU Bosch BMI323 dans le bloc de l'écran, reliée au processeur, pas au microcontrôleur de la manette : les rapports HID de la manette ne portent aucune donnée de mouvement. |
| Windows | Pilote Bosch, exposé à la pile capteurs de Windows. Lu par `Windows.Devices.Sensors.Gyrometer` (`gyro.rs`) : `RoGetActivationFactory` sur `IGyrometerStatics` (`83B6E7C9-E49D-4B39-86E6-CD554BE4C5C1`), `GetDefault` (case 6), puis `IGyrometer` : `GetCurrentReading` (6), `MinimumReportInterval` (7), `put_ReportInterval` (8) ; `IGyrometerReading` : `AngularVelocityX/Y/Z` (7, 8, 9), `f64` en degrés par seconde. Vtables écrites à la main (pas de crate de projection), `combase.dll` importée en `raw-dylib`, le thread rejoint d'abord le MTA. |
| Repère | Celui des capteurs Windows pour l'orientation native (paysage) : X vers la droite le long de l'écran, Y vers le haut de l'écran, Z sortant de l'écran. **À vérifier sur l'appareil**, avec les signes. |
| Fréquence | Environ 100 Hz d'après des relevés d'utilisateurs ; l'intervalle minimal est journalisé (`gyro: Windows gyrometer, minimum report interval … ms`). |

**Non testé sur l'appareil.** L'étude et les voies écartées (stick droit par une manette virtuelle, commande du firmware de la manette) sont dans [`gyro-study.md`](gyro-study.md).

### ACL du pipe

Le SID AppContainer du widget est dérivé par Windows (`DeriveAppContainerSidFromAppContainerName`) de son nom de famille de paquet, que Canapé calcule à partir de `widget/CanapeWidget/Package.appxmanifest` : `Canape.Widget` + `_` + le hachage de l'éditeur `CN=Canape Dev` (SHA-256 de l'éditeur en UTF-16LE, 8 premiers octets, 13 caractères en base32 de Crockford), soit `Canape.Widget_84zrj1c9kn010`, le nom observé sur l'Ally (un test unitaire le vérifie). Si la dérivation échoue, le service revient à `AC` et journalise pourquoi. Changer l'éditeur du widget change le SID : mettre à jour `WIDGET_PUBLISHER` dans `crates/canape-ipc/src/acl.rs`.

### Pas encore implémenté

| Fonction | Interface prévue |
|---|---|
| Effets d'éclairage (respiration, cycle de couleurs), une couleur par anneau, éclairage en veille ou éteint | Messages Aura (`0xB3`, `0xB5`, `0xB4`) et les drapeaux d'alimentation `0xD1 0x09` ; les couleurs des anneaux pourraient aussi différer par zone dans `0xD1 0x08`. |
| Réassignation des autres boutons (paires 1 à 7 et 9), associations secondaires, combinaisons de touches, turbo, zones mortes | Même commande `0xD1 0x02`. Les valeurs d'usine des autres paires diffèrent entre les modes manette et clavier / souris (et entre références pour les associations secondaires) : à capturer d'abord sur l'Ally. |
| Gyroscope sur le stick droit | Un pilote de manette virtuelle, ou une commande de la manette documentée nulle part : voir [`gyro-study.md`](gyro-study.md) |

La fréquence de l'écran, les boutons Armoury / Command Center, les boutons arrière et une couleur fixe des anneaux sont faits : voir [La page Matériel](#la-page-matériel).

## Conventions

Tirées de [`CLAUDE.md`](../CLAUDE.md) :

| Règle | Détail |
|---|---|
| Langues | Documentation et échanges en français (cette documentation existe aussi en anglais) ; code, commentaires et messages de commit en anglais. |
| Légèreté d'abord | Pas de navigateur embarqué, pas de runtime async sans besoin mesuré, dépendances minimales. Objectifs : service < 10 Mo et ≈ 0 % de CPU au repos, shell < 80 Mo. Mesuré en release (M4) : service 1,3 à 1,4 Mo privés, shell 10,9 à 11,3 Mo privés et 0,31 % d'un cœur au premier plan. |
| Privilèges | Seul `canape-service` a des droits élevés. `canape-shell` reste `asInvoker` : le FSE l'active en medium IL. |
| Avant de pousser | `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo check --target x86_64-pc-windows-msvc --all-targets` depuis Linux. |
| Secrets | Jamais de `.pfx` / `.cer` / `.msix` dans le dépôt (`.gitignore`). |

## Travailler à deux sessions : l'Ally et le cloud

Deux sessions travaillent sur le dépôt et se synchronisent par GitHub :

| Session | Machine | Peut | Ne peut pas |
|---|---|---|---|
| **Ally** | ROG Ally, Windows | Compiler sous Windows, installer, tester le matériel, le FSE, la Game Bar, mesurer mémoire et CPU | — |
| **Cloud** | Linux, sans matériel | Écrire le code, les tests, la documentation ; lancer le simulateur, faire des captures Xvfb ; `cargo check` pour Windows | Exécuter du code Windows, toucher au matériel |

```mermaid
sequenceDiagram
    participant Cloud as Session cloud (Linux)
    participant GH as GitHub (CI)
    participant Ally as Session Ally (Windows)
    Cloud->>Cloud: code + tests + simulateur + cargo check Windows
    Cloud->>GH: push
    GH->>GH: CI Linux et Windows, artefact canape-windows-x64
    Ally->>GH: pull, ou téléchargement de l'artefact
    Ally->>Ally: install-dev.ps1, essais sur le matériel, mesures
    Ally->>GH: corrections + docs/mN-report.md
    GH->>Cloud: pull, lecture du rapport
```

- **Ne jamais affirmer qu'un comportement Windows fonctionne sans l'avoir testé sur l'Ally.** Ce qui n'a été vérifié que sous Linux est marqué comme tel (voir l'[état d'avancement](../README.md#état-davancement)).
- Les essais sur l'Ally donnent lieu à un **rapport d'étape** (`docs/m1-report.md` à `docs/m4-report.md`) : configuration testée et commit de départ, ce qui marche, ce qui a été corrigé, mesures de mémoire et de CPU, problèmes trouvés, ce qui reste ouvert. Les preuves sont datées et chiffrées.
