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


| Fonction | Interface |
|---|---|
| Mode de performance, ventilateurs, limites de puissance (SPL/SPPT/FPPT), charge de la batterie | Firmware ASUS via le périphérique ACPI `ATKACPI` (méthodes DEVS/DSTS) |
| Limites de puissance en secours | SMU du Ryzen (approche ryzenadj) |
| Fréquence et luminosité de l'écran | API d'affichage Windows |
| Boutons spéciaux (Armoury Crate, Command Center), gyroscope | HID de la manette intégrée |


## Auto TDP (`canape-autotdp`)

Boucle d'environ une seconde dans le service :

1. **Mesure** : FPS du jeu au premier plan via ETW (même principe que PresentMon, sans injection, donc sans risque avec l'anti-triche), charge CPU/GPU, températures.
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

Interface du shell : un moteur d'interface natif en Rust, sans navigateur embarqué. Candidat principal : **Slint** (déclaratif, faible empreinte, navigation au clavier facile à brancher sur la manette via `gilrs`). À valider par un prototype mesuré sur l'Ally avant de s'engager.

## Feuille de route

| Étape | Résultat attendu |
|---|---|
| M0 | Squelette du dépôt, protocole, algorithme d'auto TDP testé (ce commit) |
| M1 | `canape-shell.exe` « hello world » qui **démarre en FSE** sur l'Ally (paquet sparse + SCCD). C'est le plus gros risque : on le lève en premier. |
| M2 | Service Windows + pipe : lecture du statut et TDP manuel depuis le shell |
| M3 | Widget Game Bar relié au service |
| M4 | Auto TDP en conditions réelles (télémétrie ETW) |
| M5 | Interface complète : bibliothèque de jeux, profils, ventilateurs, écran, batterie |

## Questions ouvertes

- Modèle exact de l'Ally (ROG Ally, Ally X, ROG Xbox Ally / Ally X) : il fixe les bornes de TDP et les ID matériels.
- Distribution hors mode développeur : nécessiterait un SCCD autorisé par Microsoft. Hors périmètre tant que Canapé reste personnel.
- Cohabitation avec la Game Bar de Microsoft et l'overlay Xbox dans le FSE : à tester en M1.
