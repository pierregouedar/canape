<div align="center">
  <img src="packaging/identity/Assets/Square150x150Logo.png" alt="Logo Canapé" width="96">

# Canapé

**Un accueil léger, façon console, pour la ROG Ally.**<br>
**A lightweight, console-style home for the ROG Ally.**

[Français](#français) · [English](#english) · [Documentation](docs/README.md)

<img src="docs/images/fr/library.png" alt="Bibliothèque de jeux dans Canapé" width="800">
</div>

## Français

Canapé remplace l'accueil Xbox dans l'expérience plein écran de Windows. Il rassemble vos jeux, se pilote à la manette et ajuste la puissance de la console selon un objectif d'images par seconde.

[Découvrir](#fonctionnalités) · [Installer](#installer-canapé) · [Utiliser](#utiliser-canapé) · [Développer](#développement) · [Documentation](#documentation)

### Fonctionnalités

- **Bibliothèque de jeux** : retrouver et lancer les jeux Steam, Epic, GOG et Xbox au même endroit ; installer les jeux Steam, Epic et GOG possédés, avec leur progression.
- **Performance** : modes ASUS, TDP manuel ou automatique, limite de FPS et ventilateurs. Les profils par jeu s'appliquent au premier plan ; le banc d'essai compare plusieurs limites de puissance.
- **En jeu** : widget Xbox Game Bar pour consulter les mesures et modifier les principaux réglages.
- **Personnalisation** : réglages de la manette, visée au gyroscope, éclairage, couleur d'accent et interface en 13 langues.
- **Légèreté** : interface Slint à rendu logiciel par défaut, sans navigateur embarqué ; service Windows séparé pour les opérations privilégiées.

| Performance | Matériel |
|---|---|
| <img src="docs/images/fr/performance.png" alt="Auto TDP et mesures de performance" width="400"> | <img src="docs/images/fr/hardware.png" alt="Réglages matériels" width="400"> |

<sub>Captures réalisées avec le simulateur et des valeurs fictives. [Autres captures](docs/user-guide.fr.md).</sub>

### État d'avancement

> [!IMPORTANT]
> Canapé est un projet personnel en développement. Le FSE, le service, la bibliothèque Steam et Xbox, les profils, l'auto TDP, le banc et plusieurs réglages matériels ont été essayés sur une **ROG Ally RC71L**, aux versions indiquées dans les [rapports d'essai](docs/m5-report.md) et [mesures détaillées](docs/m5-report-2.md). Les modifications ultérieures et les autres modèles ne sont pas nécessairement validés.

L'installation Steam sans fenêtre et plusieurs réglages récents restent à essayer sur l'Ally ; l'installation GOG est en pause. Les choix de mise à l'échelle et de génération d'images sont enregistrés, mais **ne sont pas encore appliqués**. Les [guides](docs/README.md) précisent les limites de chaque fonction.

### Installer Canapé

Il n'existe pas encore d'installateur grand public. L'installation de développement nécessite :

- une ROG Ally sous **Windows 11 24H2 ou plus récent** et le **mode développeur** activé ;
- les **Visual Studio Build Tools** avec le SDK Windows (`makeappx` et `signtool`), ainsi que Rust avec la toolchain MSVC pour compiler localement ;
- un terminal administrateur. Le script crée un certificat auto-signé et l'ajoute aux certificats de confiance de la machine.

Dans un terminal **administrateur**, à la racine du dépôt cloné :

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1
```

Choisissez ensuite **Canapé** dans Paramètres → Jeux → Expérience plein écran, puis redémarrez. Le script installe le service et le shell ; le widget Game Bar se compile et s'installe séparément.

> [!WARNING]
> Arrêtez les autres programmes qui pilotent les mêmes réglages matériels : ils peuvent modifier les limites de puissance ou les ventilateurs en même temps que Canapé.

Le [guide d'utilisation](docs/user-guide.fr.md) détaille les prérequis, les binaires de la CI, le widget, le retour à l'accueil Xbox et la désinstallation.

### Utiliser Canapé

**LB / RB** changent de section ; la croix ou le stick déplacent la sélection ; **A** ouvre ou valide ; **B** revient en arrière. Sur la page Performance, choisissez un objectif de FPS et activez l'auto TDP ; un profil peut conserver les réglages du jeu au premier plan. Le [guide Auto TDP](docs/auto-tdp.fr.md) explique les limites et le banc d'essai.

Pour suivre l'état du service depuis Windows :

```powershell
& "C:\Program Files\Canape\canape-ctl.exe" watch
```

Les journaux se trouvent dans `%ProgramData%\Canape\service.log` et `%LOCALAPPDATA%\Canape\shell.log`. Voir le [dépannage](docs/user-guide.fr.md#dépannage).

### Développement

Le dépôt est un workspace Rust : `canape-shell` et le widget sont les interfaces, `canape-service` pilote le matériel, `canape-hw` fournit le backend Ally et le simulateur, `canape-autotdp` contient le contrôleur, et `canape-core` / `canape-ipc` partagent les types et le transport. La bibliothèque de jeux est dans `canape-library` et le client de diagnostic dans `canape-ctl`.

Sous Linux, installez Rust stable, les dépendances X11 du projet (`libxkbcommon-dev`, `libfontconfig1-dev` sur Debian / Ubuntu) et lancez le simulateur dans des terminaux séparés :

```sh
# Terminal 1 : service avec FPS simulés
CANAPE_MOCK_FPS=60 cargo run -p canape-service -- --console

# Terminal 2 : interface en fenêtre
CANAPE_WINDOWED=1 cargo run -p canape-shell

# Terminal 3 : suivi du statut
cargo run -p canape-ctl -- watch
```

L'interface graphique nécessite un serveur X. Le simulateur et la compilation croisée permettent de vérifier le code sans console ; ils ne remplacent pas les essais sur l'Ally.

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
rustup target add x86_64-pc-windows-msvc # une fois
cargo check --target x86_64-pc-windows-msvc --all-targets
```

Les builds Windows de la CI sont disponibles dans les artefacts du workflow. Le [guide du développeur](docs/developer-guide.fr.md) couvre l'architecture, le protocole, les options de compilation et le simulateur.

### Documentation

- [Guide d'utilisation](docs/user-guide.fr.md) · [Auto TDP et banc d'essai](docs/auto-tdp.fr.md) · [Guide du développeur](docs/developer-guide.fr.md)
- [Index complet et rapports d'essai](docs/README.md)

### Usage de l'IA

Canapé est développé avec l'aide d'assistants de programmation, sous la direction de son propriétaire. Le projet sert à la fois à créer un logiciel utilisable sur la ROG Ally et à éprouver l'IA sur un projet complet, avec un système imposé, du matériel réel et des contraintes de légèreté.

- **Direction** : le propriétaire définit l'architecture, les règles du dépôt, l'expérience à construire et les priorités. L'interface doit notamment se piloter à la manette ; le service Windows est séparé de l'interface et seul à accéder aux opérations privilégiées.
- **Contribution** : l'assistant écrit et fait évoluer le code, les tests et la documentation à partir des demandes et retours du propriétaire.
- **Validation** : le propriétaire arbitre les choix techniques et vérifie le comportement sur l'Ally. La compilation et les tests automatisés aident à vérifier le code, mais ne remplacent pas ces essais ; les fonctions non validées sur la console sont signalées dans la documentation.

Canapé est un projet non officiel, sans affiliation avec les fabricants ou les boutiques. Legendary et gogdl sont téléchargés à la demande pour les jeux Epic et GOG ; les durées de jeu viennent de l'API d'[IGDB.com](https://www.igdb.com/) avec vos identifiants Twitch.

---

## English

Canapé replaces the Xbox home in the Windows full-screen experience. It brings your games together, works with the gamepad and adjusts the handheld's power to reach a frame-rate target.

[Features](#features) · [Install](#install-canapé) · [Use](#using-canapé) · [Develop](#development) · [Documentation](#documentation-1)

### Features

- **Game library**: find and launch Steam, Epic, GOG and Xbox games together; install owned Steam, Epic and GOG games with progress in the library.
- **Performance**: ASUS modes, manual or automatic TDP, frame-rate limit and fans. Per-game profiles follow the foreground game; the bench compares power limits.
- **In game**: an Xbox Game Bar widget provides readings and quick access to the main settings.
- **Make it yours**: controller settings, gyro aiming, lighting, accent colour and an interface in 13 languages.
- **Lightweight**: a Slint interface with software rendering by default and no embedded browser; a separate Windows service handles privileged operations.

| Performance | Hardware |
|---|---|
| <img src="docs/images/en/performance.png" alt="Auto TDP and performance readings" width="400"> | <img src="docs/images/en/hardware.png" alt="Hardware settings" width="400"> |

<sub>Screenshots use the simulator and made-up values. [More screenshots](docs/user-guide.en.md).</sub>

### Project status

> [!IMPORTANT]
> Canapé is a personal project under development. FSE startup, the service, Steam and Xbox library, profiles, auto TDP, the bench and several hardware controls have been tried on a **ROG Ally RC71L** at the versions listed in the [test reports](docs/m5-report.md) and [detailed measurements](docs/m5-report-2.md). Later changes and other models are not necessarily validated.

Windowless Steam installs and several recent settings remain to be tried on the Ally; GOG installs are on hold. Upscaling and frame-generation choices are saved but **not yet applied**. See the [guides](docs/README.md) for feature-specific limits.

### Install Canapé

There is no consumer installer yet. The development installation requires:

- a ROG Ally running **Windows 11 24H2 or later**, with **Developer Mode** enabled;
- **Visual Studio Build Tools** with the Windows SDK (`makeappx` and `signtool`), and Rust with the MSVC toolchain if building locally;
- an administrator terminal. The script creates a self-signed certificate and adds it to the machine's trusted certificates.

From an **administrator** terminal at the root of the cloned repository:

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1
```

Choose **Canapé** in Settings → Gaming → Full screen experience, then restart. The script installs the service and shell; the Game Bar widget is built and installed separately.

> [!WARNING]
> Stop other programs that control the same hardware settings: they can change power limits or fans at the same time as Canapé.

The [user guide](docs/user-guide.en.md) covers prerequisites, CI binaries, the widget, returning to the Xbox home and uninstalling.

### Using Canapé

**LB / RB** change sections; the D-pad or stick moves the selection; **A** opens or applies; **B** goes back. On the Performance page, choose an FPS target and enable auto TDP; a profile can keep the foreground game's settings. The [Auto TDP guide](docs/auto-tdp.en.md) explains its limits and the bench.

To follow the service status on Windows:

```powershell
& "C:\Program Files\Canape\canape-ctl.exe" watch
```

Logs are stored in `%ProgramData%\Canape\service.log` and `%LOCALAPPDATA%\Canape\shell.log`. See [troubleshooting](docs/user-guide.en.md#troubleshooting).

### Development

The repository is a Rust workspace: `canape-shell` and the widget provide the interfaces, `canape-service` controls hardware, `canape-hw` contains the Ally backend and simulator, `canape-autotdp` holds the controller, and `canape-core` / `canape-ipc` share types and transport. `canape-library` manages games and `canape-ctl` is the diagnostic client.

On Linux, install stable Rust and the X11 dependencies (`libxkbcommon-dev`, `libfontconfig1-dev` on Debian / Ubuntu), then start the simulator in separate terminals:

```sh
# Terminal 1: service with simulated FPS
CANAPE_MOCK_FPS=60 cargo run -p canape-service -- --console

# Terminal 2: windowed interface
CANAPE_WINDOWED=1 cargo run -p canape-shell

# Terminal 3: watch the status
cargo run -p canape-ctl -- watch
```

The graphical interface needs an X server. The simulator and cross-compilation help check code without hardware; they do not replace tests on the Ally.

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
rustup target add x86_64-pc-windows-msvc # once
cargo check --target x86_64-pc-windows-msvc --all-targets
```

Windows CI builds are available as workflow artifacts. The [developer guide](docs/developer-guide.en.md) covers the architecture, protocol, build options and simulator.

### Documentation

- [User guide](docs/user-guide.en.md) · [Auto TDP and bench](docs/auto-tdp.en.md) · [Developer guide](docs/developer-guide.en.md)
- [Full index and test reports](docs/README.md)

### Use of AI

Canapé is developed with the help of coding assistants, under its owner's direction. The project aims both to build software for the ROG Ally and to put AI to the test on a complete project, with a fixed platform, real hardware and strict size constraints.

- **Direction**: the owner sets the architecture, repository rules, user experience and priorities. The interface is designed for a gamepad; the Windows service is separate from the interface and is the only part that handles privileged operations.
- **Contribution**: the assistant writes and revises code, tests and documentation in response to the owner's requests and feedback.
- **Validation**: the owner makes technical decisions and checks behaviour on the Ally. Builds and automated tests help verify the code, but do not replace hardware testing; features not validated on the console are marked in the documentation.

Canapé is an unofficial project, not affiliated with the manufacturers or stores. Legendary and gogdl are downloaded on demand for Epic and GOG. Game times use the [IGDB.com](https://www.igdb.com/) API with your Twitch credentials.
