# Documentation de Canapé · Canapé documentation

[← README du projet / Project README](../README.md)

Toute la documentation du projet, en français et en anglais. Les rapports d'étape et l'analyse du FSE sont des documents de travail, écrits en anglais.

All the project's documentation, in French and English. The milestone reports and the FSE analysis are working documents, written in English.

## Guides

| | 🇫🇷 Français | 🇬🇧 English | Pour qui · For whom |
|---|---|---|---|
| 📖 **Utilisation · Usage** | [Guide d'utilisation](user-guide.fr.md) | [User guide](user-guide.en.md) | Installer sur l'Ally, choisir l'accueil du FSE, la bibliothèque de jeux et l'installation des jeux Steam et Epic, chaque page du shell (dont Matériel : ventilateurs, limite de FPS, manette), le widget, les langues, le dépannage, la désinstallation · Installing on the Ally, picking the FSE home app, the game library and installing Steam and Epic games, every shell page (Hardware included: fans, frame rate limit, controller), the widget, languages, troubleshooting, uninstalling |
| 🤖 **Auto TDP** | [Auto TDP et banc d'essai](auto-tdp.fr.md) | [Auto TDP and bench](auto-tdp.en.md) | L'algorithme pas à pas, ses valeurs par défaut, les profils par jeu (ventilateurs et limite de FPS compris), la limite de FPS face à l'auto TDP, le banc d'essai, les mesures, les conseils de réglage · The algorithm step by step, its defaults, per-game profiles (fans and frame rate limit included), the frame rate limit with auto TDP, the bench, measurements, tuning tips |
| 🛠️ **Développement · Development** | [Guide du développeur](developer-guide.fr.md) | [Developer guide](developer-guide.en.md) | Architecture, protocole IPC, compilation sous Linux et Windows, CI, empaquetage, widget, traductions, matériel, conventions · Architecture, IPC protocol, building on Linux and Windows, CI, packaging, widget, translations, hardware, conventions |

## Documents de référence · Reference documents

| Document | Langue · Language | Contenu · Contents |
|---|---|---|
| [`game-install-study.md`](game-install-study.md) | 🇫🇷 | Étude et état : installer les jeux possédés depuis Canapé sans ouvrir les launchers. Steam et Epic (via Legendary) sont codés, pas encore essayés sur l'Ally ; GOG et Xbox restent ouverts. · Study and status: installing owned games from Canapé without the store launchers. Steam and Epic (through Legendary) are coded, not tried on the Ally yet; GOG and Xbox are still open. |
| [`gyro-study.md`](gyro-study.md) | 🇫🇷 | Étude et état : la visée au gyroscope. Gyroscope → souris depuis le shell, sans pilote, codé, pas encore essayé sur l'Ally ; le stick droit reste ouvert. · Study and status: gyro aiming. Gyroscope → mouse from the shell, without a driver, coded, not tried on the Ally yet; the right stick is still open. |
| [`howlongtobeat-study.md`](howlongtobeat-study.md) | 🇫🇷 | Étude : la durée de jeu (HowLongToBeat) sur la fiche du jeu. HowLongToBeat écarté (sans API officielle, l'accès au site casse tous les 2 à 3 mois) ; IGDB retenu et implémenté. · Study: time to beat (HowLongToBeat) on the game page. HowLongToBeat ruled out (no official API, access to the site breaks every 2 to 3 months); IGDB chosen and implemented. |
| [`gpu-renderer-test.md`](gpu-renderer-test.md) | 🇫🇷 | Essai sur l'Ally : rendu logiciel face à OpenGL et Direct3D 12 (mémoire, FPS de l'accueil, CPU, effet en jeu) ; on garde le rendu logiciel. · Ally test: software rendering against OpenGL and Direct3D 12 (memory, home page FPS, CPU, in-game effect); software rendering stays. |
| [`architecture.md`](architecture.md) | 🇫🇷 | Document de conception initial : composants, démarrage en FSE, IPC, matériel, objectifs de légèreté, feuille de route. Sa feuille de route n'est plus à jour : voir l'[état d'avancement](../README.md#état-davancement). · Initial design document; its roadmap is out of date, see the [project status](../README.md#project-status). |
| [`../packaging/README.md`](../packaging/README.md) | 🇫🇷 | Fichiers du paquet d'identité, script d'installation, pièges de Windows. · Identity package files, install script, Windows pitfalls. |
| [`../widget/README.md`](../widget/README.md) | 🇫🇷 | Choix techniques du widget Game Bar, interface, ajout d'une langue, compilation. · Game Bar widget technical choices, UI, adding a language, building. |
| [`../CLAUDE.md`](../CLAUDE.md) | 🇫🇷 | Règles du projet pour les sessions de développement. · Project rules for development sessions. |

## Rapports d'étape · Milestone reports

Comptes rendus des essais sur la ROG Ally RC71L : ce qui marche, ce qui a été corrigé, les mesures, les problèmes, ce qui reste ouvert.

Reports of the tests on the ROG Ally RC71L: what works, what was fixed, measurements, problems, what is still open.

| Rapport · Report | Commit de départ · Starting commit | Résumé · Summary |
|---|---|---|
| [M1](m1-report.md) | M0 (`6b0bc20`) | Le paquet d'identité s'installe ; le FSE lance `canape-shell.exe` au démarrage via `windows.gaming:///home`. Smart App Control repéré. · The identity package installs; the FSE launches `canape-shell.exe` at boot through `windows.gaming:///home`. Smart App Control spotted. |
| [M2](m2-report.md) | `cad1d56` | Service, backend ATKACPI vérifié, TDP effectif, shell Slint à la manette, instance unique, boot FSE OK ; correctif des statuts poussés sous Windows. · Service, ATKACPI backend checked, TDP takes effect, gamepad Slint shell, single instance, FSE boot OK; fix for status pushes on Windows. |
| [M3](m3-report.md) | `2ccedd6` | Widget Game Bar compilé, installé et relié au service ; pipe ouvert depuis l'AppContainer. · Game Bar widget built, installed and connected to the service; pipe opened from the AppContainer. |
| [M5, 1ʳᵉ partie · part 1](m5-report.md) | `6c22fc1`, correctifs `ed4e118`, `64462cd` | Shell, page Matériel, boutons, bibliothèque et bench sur l'Ally ; auto TDP en dents de scie corrigé, jeux Steam en double, limite de FPS ; prochains bancs avec Unigine Heaven et FurMark 2. · Shell, Hardware page, buttons, library and bench on the Ally; low-target sawtooth fixed, duplicated Steam games, frame rate limit; next benches with Unigine Heaven and FurMark 2. |
| [M2.5 / M4](m4-report.md) | `94a4db0`, retest `13ee476` | Persistance, conflits, build release, mémoire (service 1,4 Mo, shell 11 Mo), ADL depuis la session 0, auto TDP réel : fin des dents de scie. · Persistence, conflicts, release build, memory (service 1.4 MB, shell 11 MB), ADL from session 0, real auto TDP: sawtooth fixed. |

## Captures · Screenshots

Rendues sous Linux avec le simulateur matériel et des valeurs fictives (voir [Captures d'écran avec Xvfb](developer-guide.fr.md#captures-décran-avec-xvfb)). · Rendered on Linux with the hardware simulator and made-up values (see [Screenshots with Xvfb](developer-guide.en.md#screenshots-with-xvfb)).

| Fichier · File | Page | Langue · Language |
|---|---|---|
| [`fr/library.png`](images/fr/library.png) | Bibliothèque (jeux inventés) | Français |
| [`fr/library-install.png`](images/fr/library-install.png) | Bibliothèque, jeux à installer (installation simulée) | Français |
| [`fr/performance.png`](images/fr/performance.png) | Performance | Français |
| [`fr/hardware.png`](images/fr/hardware.png) | Matériel | Français |
| [`fr/profiles.png`](images/fr/profiles.png) | Profils | Français |
| [`fr/bench-results.png`](images/fr/bench-results.png) | Banc d'essai (résultats) | Français |
| [`fr/bench-running.png`](images/fr/bench-running.png) | Banc d'essai en cours | Français |
| [`fr/settings.png`](images/fr/settings.png) | Réglages (langue, boutons de l'Ally, jaquettes, compte Epic, couleur d'accent) | Français |
| [`fr/add-game.png`](images/fr/add-game.png) | Ajouter un jeu (raccourcis inventés) | Français |
| [`en/library.png`](images/en/library.png) | Library (made-up games) | English |
| [`en/library-install.png`](images/en/library-install.png) | Library, games to install (simulated install) | English |
| [`en/performance.png`](images/en/performance.png) | Performance | English |
| [`en/hardware.png`](images/en/hardware.png) | Hardware | English |
| [`en/profiles.png`](images/en/profiles.png) | Profiles | English |
| [`en/bench-results.png`](images/en/bench-results.png) | Bench (results) | English |
| [`en/bench-running.png`](images/en/bench-running.png) | Bench running | English |
| [`en/settings.png`](images/en/settings.png) | Settings (language, Ally buttons, covers, Epic account, accent colour) | English |
| [`en/add-game.png`](images/en/add-game.png) | Add a game (made-up shortcuts) | English |
| [`ja/library.png`](images/ja/library.png) | ゲーム | 日本語 |
| [`ru/performance.png`](images/ru/performance.png) | Производительность | Русский |
| [`de/settings.png`](images/de/settings.png) | Einstellungen | Deutsch |
| [`accent-teal.png`](images/accent-teal.png) | Performance, accent sarcelle · teal accent | English |
| [`accent-amber.png`](images/accent-amber.png) | Performance, accent orange · orange accent | English |
