# Architecture de Canapé

Canapé est une interface « console » pour la ROG Ally : elle démarre à la place de l'accueil Xbox dans l'expérience plein écran (FSE) de Windows, pilote tout le matériel de l'Ally et ajuste le TDP automatiquement. Deux contraintes guident tous les choix : **booter en FSE** et **rester le plus léger possible**.


## Vue d'ensemble

```
                    ┌──────────────────────────── session utilisateur (medium IL) ─┐
 Windows FSE ──────►│ canape-shell.exe  (app d'accueil FSE, Rust)                   │
 (GamingHomeApp)    │   identité : paquet MSIX « sparse » Canape                    │
                    │                                                               │
 Xbox Game Bar ────►│ Widget Canapé  (coquille UWP minimale, AppContainer)          │
                    └───────────────┬───────────────────────────────┬───────────────┘
                                    │ named pipe \\.\pipe\canape    │
                                    │ (JSON, une ligne par message) │
                    ┌───────────────▼───────────────────────────────▼───────────────┐
                    │ canape-service.exe  (service Windows, LocalSystem, Rust)      │
                    │   ├─ canape-hw       : ASUS ACPI/WMI, puissance, écran, HID   │
                    │   ├─ canape-autotdp  : boucle d'auto TDP                      │
                    │   └─ télémétrie      : FPS (ETW), charges, températures       │
                    └───────────────────────────────────────────────────────────────┘
```

Seul le service a des droits élevés. L'app FSE et le widget ne touchent jamais le matériel : ils envoient des requêtes au service.

## Composants

| Composant | Langage | Rôle | Démarrage |
|---|---|---|---|
| `canape-service` | Rust | Matériel, profils, auto TDP, serveur IPC | Service Windows automatique (au boot, avant la session) |
| `canape-shell` | Rust | Interface plein écran à la manette : bibliothèque de jeux, réglages | Activé par le FSE comme app d'accueil |
| Widget Game Bar | C# (UWP), le plus fin possible | Réglages rapides en jeu : TDP, mode, FPS cible, luminosité | Ouvert par l'utilisateur dans la Game Bar |
| `canape-core` | Rust | Protocole IPC, types partagés, configuration | Bibliothèque |
| `canape-hw` | Rust | Abstraction du matériel, backend Ally sous Windows | Bibliothèque |
| `canape-autotdp` | Rust | Algorithme d'auto TDP, sans dépendance au système | Bibliothèque |
| `canape-ipc` | Rust | Transport : named pipe sous Windows, socket Unix ailleurs (dev) | Bibliothèque |
| `canape-ctl` | Rust | Client en ligne de commande, pour tester le service | Outil |

### Pourquoi un vrai service plutôt que des tâches planifiées


## Démarrage en FSE


1. `canape-shell.exe` est installé normalement dans `C:\Program Files\Canape`.
2. Un **paquet MSIX sparse** (`packaging/identity`) lui donne une identité d'app :
   - `uap10:AllowExternalContent`, `TrustLevel="mediumIL"`, `RuntimeBehavior="win32App"` ;
   - extension `uap3:AppExtension Name="windows.gamingApp"` : rend l'app candidate comme accueil du FSE ;
   - capacité `Microsoft.appCategory.gamingHome_8wekyb3d8bbwe`, débloquée par `CustomCapability.SCCD` (catalogue de test `FFFF`, donc **mode développeur requis**).
3. L'installateur enregistre le paquet : `Add-AppxPackage -Path Canape.Identity.msix -ExternalLocation "C:\Program Files\Canape"`. Le paquet doit être signé ; en développement, un certificat auto-signé placé dans `LocalMachine\TrustedPeople`.
4. Choix de l'accueil : en priorité par l'utilisateur dans Paramètres → Jeux → Expérience plein écran. En option, l'installateur écrit `HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration\GamingHomeApp = <PFN>!App` (valeur non documentée par Microsoft).
5. Au boot : si « démarrer dans l'expérience plein écran » est activé (`StartupToGamingHome = 1`), Windows lance lui-même `canape-shell.exe`.

À l'exécution, le shell peut utiliser les API de `api-ms-win-gaming-experience-l1-1-0.dll` (`IsGamingFullScreenExperienceActive`, notification de changement, etc.) pour suivre l'état du FSE.

## Communication (IPC)

- **Canal** : named pipe `\\.\pipe\canape`, ouvert par le service.
- **Format** : JSON, un message par ligne, défini dans `canape-core::protocol`. Chaque client commence par `Hello { protocol_version }`.
- **Sécurité** : la DACL du pipe autorise SYSTEM, les administrateurs, l'utilisateur interactif et le SID du paquet du widget (AppContainer). Personne d'autre ne peut piloter le matériel.
- **Notifications** : le service pousse un `Status` (TDP, FPS, températures, batterie) environ une fois par seconde aux clients abonnés.

## Matériel (`canape-hw`)


| Fonction | Interface | État |
|---|---|---|
| Mode de performance, limites de puissance (sPPT, SPL, fPPT), limite de charge, température CPU | Firmware ASUS via `\\.\ATKACPI` (IOCTL `0x0022240C`, méthodes DEVS/DSTS) | M2 |
| Batterie (pourcentage) | `GetSystemPowerStatus` | M2 |
| Courbes de ventilateurs | ATKACPI `0x00110024`/`0x00110025` (8 températures + 8 % de PWM) | M5 |
| Fréquence de l'écran | `ChangeDisplaySettingsEx` sur l'écran interne | M5 |
| Mode de la manette, boutons spéciaux | HID ASUS (VID `0x0B05`, rapport `0x5A`) | M5 |




## Auto TDP (`canape-autotdp`)

Boucle d'environ une seconde dans le service :

2. **Décision** : objectif = FPS cible (par exemple 60) avec le moins de watts possible.
   - FPS sous la cible (au-delà d'une marge) → on monte le TDP ;
   - FPS au-dessus de la cible avec de la marge → on baisse le TDP par petits pas ;
   - un délai minimal entre deux changements évite les oscillations.
3. **Action** : nouvelle limite de puissance via `canape-hw`, bornée par le modèle (par exemple 7 à 30 W).

L'algorithme est une fonction pure, testée sans matériel. Les paramètres (cible, bornes, pas) sont réglables par profil de jeu.

## Légèreté : objectifs chiffrés

| Composant | Mémoire visée | CPU au repos |
|---|---|---|
| `canape-service` | < 10 Mo | < 0,5 % |
| `canape-shell` | < 80 Mo | ≈ 0 % hors animation |
| Widget | ce qu'impose la Game Bar | ≈ 0 % |

Interface du shell : **Slint** avec son **rendu logiciel**, sans navigateur embarqué ni contexte OpenGL (le pilote OpenGL coûtait l'essentiel des 115 Mo mesurés en M2). La manette est lue en XInput direct, seulement quand le shell est au premier plan, et traduite en touches Tab / Maj+Tab / Entrée avec répétition quand une direction est maintenue. L'écran est pensé pour 1280×720 points logiques (1080p à 150 %).

Le service mémorise les derniers réglages appliqués (`%ProgramData%\Canape\state.json`) et les réapplique au démarrage : le firmware ne sait pas les relire. L'auto TDP ne bouge que si un jeu tourne (au moins 5 FPS et 15 % d'activité GPU), sinon il monterait au maximum devant le bureau.

## Feuille de route

| Étape | Résultat attendu |
|---|---|
| M0 ✅ | Squelette du dépôt, protocole, algorithme d'auto TDP testé |
| M1 ✅ | `canape-shell.exe` **démarre en FSE** sur l'Ally (voir `m1-report.md`) |
| M2 ✅ | Service Windows + pipe, backend ATKACPI, interface Slint à la manette, instance unique (voir `m2-report.md`) |
| M2.5 🚧 | État persistant et réappliqué au démarrage, journal des écritures, détection des programmes concurrents, rendu logiciel, manette en XInput direct, CI Windows |
| M3 | Widget Game Bar relié au service |
| M4 🚧 | Auto TDP en conditions réelles : FPS et charge GPU via AMD ADL |
| M5 | Interface complète : bibliothèque de jeux, profils, ventilateurs, écran, batterie |

## Questions ouvertes

- Modèle exact de l'Ally (ROG Ally, Ally X, ROG Xbox Ally / Ally X) : il fixe les bornes de TDP et les ID matériels.
- Distribution hors mode développeur : nécessiterait un SCCD autorisé par Microsoft. Hors périmètre tant que Canapé reste personnel.
- Cohabitation avec la Game Bar de Microsoft et l'overlay Xbox dans le FSE : à tester en M1.
