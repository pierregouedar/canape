<div align="center">

# Canapé

**Un accueil léger, façon console, pour la ROG Ally.**<br>
**A light, console-style home for the ROG Ally.**

[Français](#français) · [English](#english) · [Documentation](docs/README.md)

<img src="docs/images/fr/library.png" alt="Bibliothèque de jeux Canapé" width="800">

</div>

## Français

Canapé remplace l'accueil Xbox dans l'expérience plein écran de Windows. Sa bibliothèque réunit vos jeux et son interface se pilote à la manette. Un service Windows séparé gère le matériel et ajuste la puissance selon votre objectif de FPS.

### Vos jeux, votre console

- **Bibliothèque** : jeux Steam, Epic, GOG et Xbox au même endroit, avec jaquettes et lancement à la manette. Les jeux Steam, Epic et GOG possédés peuvent aussi être installés depuis Canapé.
- **Performance** : modes ASUS, TDP manuel ou automatique, limite de FPS, ventilateurs et profils par jeu. Un banc d'essai compare plusieurs limites de puissance.
- **En jeu** : le widget Xbox Game Bar donne accès aux réglages et aux mesures sans quitter la partie.
- **À votre main** : réglages de la manette, gyroscope, éclairage, couleur d'accent et interface en 13 langues.

| Performance | Matériel |
|---|---|
| <img src="docs/images/fr/performance.png" alt="Auto TDP et mesures de performance" width="400"> | <img src="docs/images/fr/hardware.png" alt="Réglages matériels" width="400"> |

<sub>Captures réalisées avec le simulateur, à partir de valeurs fictives. [Voir les autres captures](docs/user-guide.fr.md).</sub>

### Installer

Canapé est un **projet personnel en développement** : l'installation demande le mode développeur de Windows et un certificat auto-signé. Il n'y a pas encore d'installateur grand public.

1. Activez le **mode développeur** sur l'Ally.
2. Dans un terminal **administrateur**, à la racine du dépôt :

   ```powershell
   powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1
   ```

3. Choisissez **Canapé** dans Paramètres → Jeux → Expérience plein écran, puis redémarrez.

Le [guide d'utilisation](docs/user-guide.fr.md) couvre l'installation, le widget, le dépannage et la désinstallation.

### État d’avancement

Le démarrage FSE, le service, la bibliothèque Steam et Xbox, les profils, l'auto TDP, le banc et plusieurs réglages matériels ont été essayés sur une **ROG Ally RC71L**, aux versions indiquées dans les [rapports](docs/m5-report.md) et [essais détaillés](docs/m5-report-2.md). Les versions ultérieures et les autres modèles ne sont pas nécessairement validés.

L'installation Steam sans fenêtre, les installations Epic et GOG, la visée au gyroscope et plusieurs réglages récents restent à essayer sur l'Ally. Les choix de mise à l'échelle et de génération d'images sont enregistrés, mais pas encore appliqués. Consultez les [guides](docs/README.md) pour les limites de chaque fonction.

### Sous le capot

Canapé privilégie la légèreté : Rust, interface Slint avec rendu logiciel par défaut, sans navigateur embarqué. L'interface fonctionne sans droits élevés ; seul le service accède aux réglages privilégiés. Une version release a été mesurée à **1,4 Mo** de mémoire privée pour le service et **11 Mo** pour l'interface ([mesures et version](docs/m4-report.md)).

- [Guide du développeur](docs/developer-guide.fr.md) · [Auto TDP et banc d'essai](docs/auto-tdp.fr.md) · [Documentation complète](docs/README.md)
- Projet développé avec l'aide d'outils d'IA, sous la direction et avec les essais matériels du propriétaire.
- Projet non officiel, sans affiliation avec les fabricants ou les boutiques. Les outils Legendary et gogdl sont téléchargés à la demande pour Epic et GOG. Les durées de jeu viennent de l’API d’[IGDB.com](https://www.igdb.com/) avec vos identifiants Twitch. Licence [MIT](LICENSE).

---

## English

Canapé replaces the Xbox home in the Windows full-screen experience. Its gamepad-friendly library brings your games together, while a separate Windows service controls the hardware and adjusts power to meet your FPS target.

### Your games, your handheld

- **Library**: Steam, Epic, GOG and Xbox games in one place, with covers and gamepad launch. Owned Steam, Epic and GOG games can also be installed from Canapé.
- **Performance**: ASUS modes, manual or automatic TDP, frame-rate limit, fans and per-game profiles. A bench compares power limits.
- **In game**: the Xbox Game Bar widget puts settings and readings within reach without leaving the game.
- **Make it yours**: controller settings, gyro aiming, lighting, accent colour and an interface in 13 languages.

| Performance | Hardware |
|---|---|
| <img src="docs/images/en/performance.png" alt="Auto TDP and performance readings" width="400"> | <img src="docs/images/en/hardware.png" alt="Hardware settings" width="400"> |

<sub>Screenshots use the hardware simulator and made-up values. [More screenshots](docs/user-guide.en.md).</sub>

### Install

Canapé is a **personal project under development**. Installation requires Windows Developer Mode and a self-signed certificate; there is no consumer installer yet.

1. Enable **Developer Mode** on the Ally.
2. In an **administrator** terminal at the repository root:

   ```powershell
   powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1
   ```

3. Choose **Canapé** in Settings → Gaming → Full screen experience, then restart.

The [user guide](docs/user-guide.en.md) covers installation, the widget, troubleshooting and uninstalling.

### Project status

FSE startup, the service, Steam and Xbox library, profiles, auto TDP, the bench and several hardware controls have been tried on a **ROG Ally RC71L** at the versions listed in the [reports](docs/m5-report.md) and [detailed tests](docs/m5-report-2.md). Later versions and other models are not necessarily validated.

Windowless Steam installation, Epic and GOG installation, gyro aiming and several recent settings remain to be tried on the Ally. Upscaling and frame-generation choices are saved but not applied yet. See the [guides](docs/README.md) for each feature's limits.

### Under the hood

Canapé is built for a small footprint: Rust, a Slint interface with software rendering by default, and no embedded browser. The interface runs without elevated privileges; only the service accesses privileged settings. One release measured **1.4 MB** of private memory for the service and **11 MB** for the interface ([measurements and version](docs/m4-report.md)).

- [Developer guide](docs/developer-guide.en.md) · [Auto TDP and bench](docs/auto-tdp.en.md) · [All documentation](docs/README.md)
- Developed with AI tools, under the owner's direction and with hardware testing by the owner.
- An unofficial project, not affiliated with the manufacturers or stores. Legendary and gogdl are downloaded on demand for Epic and GOG. Game times use the [IGDB.com](https://www.igdb.com/) API with your Twitch credentials. [MIT license](LICENSE).
