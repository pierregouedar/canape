# Installer des jeux sans ouvrir les launchers : étude

Objectif : depuis la bibliothèque de Canapé, voir les jeux **possédés mais pas installés** et les installer à la manette, sans passer par l'interface de Steam, d'Epic, de GOG Galaxy ou de l'app Xbox.

Résumé : c'est **possible pour Steam et Epic**, partiellement pour GOG, et **pas pour le Game Pass / l'app Xbox**. Rien n'est codé pour l'instant : deux choix (une clé d'API Steam, des outils tiers sous GPL) reviennent au propriétaire du projet.

## Par boutique

| Boutique | Liste des jeux possédés | Installer | Suivre la progression | Sans launcher ? |
|---|---|---|---|---|
| **Steam** | API Web Steam `IPlayerService/GetOwnedGames` : une clé d'API gratuite (steamcommunity.com/dev/apikey) + le SteamID64, lu localement dans `config/loginusers.vdf`. Avec sa propre clé, ça marche même si le profil est privé. | `steam://install/<appid>` : le client Steam (lancé en arrière-plan avec `-silent` s'il ne tourne pas) ouvre seulement sa petite fenêtre « Installer » (dossier, raccourcis), puis télécharge en arrière-plan. | En local : `steamapps/appmanifest_<appid>.acf` apparaît dès le début, avec `BytesToDownload` / `BytesDownloaded` et `StateFlags`. Canapé les lit déjà pour la bibliothèque. | **Presque** : le client Steam doit tourner (il le faut de toute façon pour jouer), mais sa grande interface ne s'ouvre pas. Une seule fenêtre de confirmation, utilisable à la manette. |
| **Epic** | [Legendary](https://github.com/derrod/legendary) (`legendary list --json`). | `legendary install <app>` : téléchargement et installation sans le lanceur Epic. | Sortie de Legendary (progression en continu). | **Oui, complètement** : Legendary peut aussi lancer les jeux (`legendary launch`) et synchroniser les sauvegardes cloud. Connexion une fois par une fenêtre de login Epic (exe Windows ≥ 0.20.14), ou import de la session du lanceur Epic (`--import`). |
| **GOG** | API GOG (`embed.gog.com/user/data/games`) avec un jeton OAuth. | Deux voies : télécharger l'installateur hors ligne du jeu et le lancer en silencieux (`/VERYSILENT`), ou [heroic-gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl), le module de téléchargement de Heroic. | gogdl : sa sortie ; installateur : la taille du fichier téléchargé. | **Oui**, mais plus de travail : gogdl n'est pas fait pour être utilisé directement (« not user friendly cli »), et l'auth GOG est à intégrer nous-mêmes. |
| **Xbox / Game Pass** | Pas d'API publique pour les jeux possédés ou le catalogue Game Pass PC. | `ms-windows-store://pdp/?productid=<id>` ou l'app Xbox : ouvre la fiche, l'installation passe par Gaming Services. `winget --source msstore` installe des apps du Store, pas les jeux Game Pass. | Non exposée. | **Non** : au mieux, ouvrir la fiche du jeu dans l'app Xbox. |

## Contraintes

- **Licences** : Legendary et heroic-gogdl sont sous **GPL-3**. Canapé (MIT) peut les **appeler comme programmes séparés** sans changer de licence. Les embarquer dans notre installateur reste possible (« agrégat »), à condition de fournir leur code source et leur licence. Le plus simple : les télécharger à la demande depuis leurs releases officielles dans `%LOCALAPPDATA%\Canape\tools`.
- **Légèreté** : rien ne tourne en permanence. Legendary et gogdl sont des exe PyInstaller (~15–20 Mo chacun), lancés seulement pendant une installation. La clé Steam et les jetons restent sur l'appareil (`shell.json`, et les fichiers d'auth des outils).
- **Droits** : tout se fait dans la session de l'utilisateur, sans élévation. Le shell lance les processus, comme pour les jeux.
- **Disque** : choix du dossier d'installation (le premier dossier de bibliothèque Steam, `C:\Games\Epic`, …) et de l'espace libre à vérifier avant de lancer.

## Proposition

1. **Étape 1, Steam** (sans outil tiers) : un filtre **« À installer »** dans la bibliothèque, avec les jeux Steam possédés (clé d'API dans les Réglages), leurs jaquettes (déjà gérées par les jaquettes téléchargées), et **A** = installer. La progression vient des manifestes locaux ; le jeu bascule dans la bibliothèque à la fin.
2. **Étape 2, Epic** : Legendary téléchargé à la demande. Connexion une fois depuis Canapé (fenêtre Epic), liste des jeux possédés, installation, puis lancement direct sans le lanceur Epic.
3. **Étape 3, GOG** : installateurs hors ligne en silencieux, ou gogdl.
4. **Xbox** : un bouton qui ouvre la fiche dans l'app Xbox, sans plus.
