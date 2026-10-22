# Documentation de Canapé · Canapé documentation

[← README du projet / Project README](../README.md)

Toute la documentation du projet, en français et en anglais. Les rapports d'étape et l'analyse du FSE sont des documents de travail, écrits en anglais.

All the project's documentation, in French and English. The milestone reports and the FSE analysis are working documents, written in English.

## Guides

| | 🇫🇷 Français | 🇬🇧 English | Pour qui · For whom |
|---|---|---|---|
| 📖 **Utilisation · Usage** | [Guide d'utilisation](user-guide.fr.md) | [User guide](user-guide.en.md) | Installer sur l'Ally, choisir l'accueil du FSE, la bibliothèque de jeux, chaque page du shell (dont Matériel : ventilateurs, limite de FPS, manette), le widget, les langues, le dépannage, la désinstallation · Installing on the Ally, picking the FSE home app, the game library, every shell page (Hardware included: fans, frame rate limit, controller), the widget, languages, troubleshooting, uninstalling |
| 🤖 **Auto TDP** | [Auto TDP et banc d'essai](auto-tdp.fr.md) | [Auto TDP and bench](auto-tdp.en.md) | L'algorithme pas à pas, ses valeurs par défaut, les profils par jeu (ventilateurs et limite de FPS compris), la limite de FPS face à l'auto TDP, le banc d'essai, les mesures, les conseils de réglage · The algorithm step by step, its defaults, per-game profiles (fans and frame rate limit included), the frame rate limit with auto TDP, the bench, measurements, tuning tips |
| 🛠️ **Développement · Development** | [Guide du développeur](developer-guide.fr.md) | [Developer guide](developer-guide.en.md) | Architecture, protocole IPC, compilation sous Linux et Windows, CI, empaquetage, widget, traductions, matériel, conventions · Architecture, IPC protocol, building on Linux and Windows, CI, packaging, widget, translations, hardware, conventions |

## Documents de référence · Reference documents

| Document | Langue · Language | Contenu · Contents |
|---|---|---|
| [`game-install-study.md`](game-install-study.md) | 🇫🇷 | Étude : installer les jeux possédés depuis Canapé sans ouvrir les launchers (Steam, Epic via Legendary, GOG, Xbox), contraintes et proposition. · Study: installing owned games from Canapé without the store launchers. |
| [`architecture.md`](architecture.md) | 🇫🇷 | Document de conception initial : composants, démarrage en FSE, IPC, matériel, objectifs de légèreté, feuille de route. Sa feuille de route n'est plus à jour : voir l'[état d'avancement](../README.md#état-davancement). · Initial design document; its roadmap is out of date, see the [project status](../README.md#project-status). |
| [`../packaging/README.md`](../packaging/README.md) | 🇫🇷 | Fichiers du paquet d'identité, script d'installation, pièges de Windows. · Identity package files, install script, Windows pitfalls. |
| [`../widget/README.md`](../widget/README.md) | 🇫🇷 | Choix techniques du widget Game Bar, interface, ajout d'une langue, compilation. · Game Bar widget technical choices, UI, adding a language, building. |
| [`../CLAUDE.md`](../CLAUDE.md) | 🇫🇷 | Règles du projet pour les sessions de développement. · Project rules for development sessions. |

## Rapports d'étape · Milestone reports

Comptes rendus des essais sur la ROG Ally RC71L : ce qui marche, ce qui a été corrigé, les mesures, les problèmes, ce qui reste ouvert.

Reports of the tests on the ROG Ally RC71L: what works, what was fixed, measurements, problems, what is still open.

| Rapport · Report | Commit de départ · Starting commit | Résumé · Summary |
|---|---|---|
| [M1](m1-report.md) | M0 (`16819fa`) | Le paquet d'identité s'installe ; le FSE lance `canape-shell.exe` au démarrage via `windows.gaming:///home`. Smart App Control repéré. · The identity package installs; the FSE launches `canape-shell.exe` at boot through `windows.gaming:///home`. Smart App Control spotted. |
| [M3](m3-report.md) | `c24e6e4` | Widget Game Bar compilé, installé et relié au service ; pipe ouvert depuis l'AppContainer. · Game Bar widget built, installed and connected to the service; pipe opened from the AppContainer. |
| [M2.5 / M4](m4-report.md) | `c617cf3`, retest `99cad07` | Persistance, conflits, build release, mémoire (service 1,4 Mo, shell 11 Mo), ADL depuis la session 0, auto TDP réel : fin des dents de scie. · Persistence, conflicts, release build, memory (service 1.4 MB, shell 11 MB), ADL from session 0, real auto TDP: sawtooth fixed. |

## Captures · Screenshots

Rendues sous Linux avec le simulateur matériel et des valeurs fictives (voir [Captures d'écran avec Xvfb](developer-guide.fr.md#captures-décran-avec-xvfb)). · Rendered on Linux with the hardware simulator and made-up values (see [Screenshots with Xvfb](developer-guide.en.md#screenshots-with-xvfb)).

| Fichier · File | Page | Langue · Language |
|---|---|---|
| [`fr/library.png`](images/fr/library.png) | Bibliothèque (jeux inventés) | Français |
| [`fr/home.png`](images/fr/home.png) | Accueil | Français |
| [`fr/performance.png`](images/fr/performance.png) | Performance | Français |
| [`fr/hardware.png`](images/fr/hardware.png) | Matériel | Français |
| [`fr/profiles.png`](images/fr/profiles.png) | Profils | Français |
| [`fr/bench-results.png`](images/fr/bench-results.png) | Banc d'essai (résultats) | Français |
| [`fr/bench-running.png`](images/fr/bench-running.png) | Banc d'essai en cours | Français |
| [`fr/settings.png`](images/fr/settings.png) | Réglages (couleur d'accent) | Français |
| [`en/library.png`](images/en/library.png) | Library (made-up games) | English |
| [`en/home.png`](images/en/home.png) | Home | English |
| [`en/performance.png`](images/en/performance.png) | Performance | English |
| [`en/hardware.png`](images/en/hardware.png) | Hardware | English |
| [`en/profiles.png`](images/en/profiles.png) | Profiles | English |
| [`en/bench-results.png`](images/en/bench-results.png) | Bench (results) | English |
| [`en/bench-running.png`](images/en/bench-running.png) | Bench running | English |
| [`en/settings.png`](images/en/settings.png) | Settings (accent colour) | English |
| [`ja/home.png`](images/ja/home.png) | ホーム | 日本語 |
| [`ru/performance.png`](images/ru/performance.png) | Производительность | Русский |
| [`de/settings.png`](images/de/settings.png) | Einstellungen | Deutsch |
| [`accent-teal.png`](images/accent-teal.png) | Home, accent sarcelle · teal accent | English |
| [`accent-amber.png`](images/accent-amber.png) | Home, accent ambre · amber accent | English |
