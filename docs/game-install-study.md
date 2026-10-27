# Installer des jeux sans ouvrir les launchers : étude

Objectif : depuis la bibliothèque de Canapé, voir les jeux **possédés mais pas installés** et les installer à la manette, sans passer par l'interface de Steam, d'Epic, de GOG Galaxy ou de l'app Xbox.

Résumé : l'installation Steam, Epic et GOG est codée ; le repli Steam par sa fenêtre a été essayé sur l'Ally. L'installation sans fenêtre, Epic et GOG restent à essayer sur l'Ally. L'installation Game Pass / Xbox reste ouverte.

## État

| Étape | État |
|---|---|
| 1. Steam | Code écrit : liste par l'API Web (clé `steam_api_key`), ajout à la file Steam sans fenêtre avec repli sur `steam://install/<appid>`, progression lue dans le manifeste. Le repli et la liste ont été essayés sur l'Ally ([rapport](m5-report-2.md#steam-install)) ; l'installation sans fenêtre reste à essayer. |
| 2. Epic | 🧪 Code écrit (`778a222`) : Legendary téléchargé à la demande dans `%LOCALAPPDATA%\Canape\tools` (vérifié contre le SHA-256 publié par GitHub), connexion depuis la bibliothèque ou les Réglages, liste, installation avec progression, lancement par `legendary launch`. **Pas encore essayé sur l'Ally.** |
| 3. GOG | 🧪 Code écrit : gogdl (heroic-gogdl 1.3.1, version fixée, SHA-256 vérifié) téléchargé à la demande dans `%LOCALAPPDATA%\Canape\tools` ; connexion par la page GOG ouverte dans le navigateur, dont Canapé lit l'adresse d'arrivée dans le presse-papiers ; liste par l'API web de GOG avec le jeton de gogdl ; installation par `gogdl download` avec progression ; lancement direct par l'exécutable du jeu (`goggame-<id>.info`). Les réponses de GOG n'ont pas pu être enregistrées (serveurs injoignables depuis la session cloud) : fixtures écrites d'après le code de gogdl. **Pas encore essayé sur l'Ally.** |
| 4. Xbox | ⏳ Ouvert. |

Dossier d'installation et place libre (code écrit, à vérifier sur l'Ally) : Steam choisit la bibliothèque ayant le plus de place, ou propose le dossier dans sa fenêtre de repli. Pour Epic et GOG, un **dossier des jeux** (réglage `games_folder` dans `shell.json`, choisi dans les Réglages) reçoit Legendary et gogdl. La fiche montre la taille et la place libre ; l'installation refuse si le jeu ne rentre pas et revérifie avant téléchargement.

Les deux choix laissés au propriétaire du projet ont été tranchés ainsi : une clé d'API Steam personnelle, renseignée à la main dans `shell.json` ; Legendary téléchargé à la demande, jamais embarqué. Le fonctionnement est décrit dans le [guide d'utilisation](user-guide.fr.md#installer-des-jeux-possédés) et le [guide du développeur](developer-guide.fr.md#installer-les-jeux-possédés). Le reste de ce document est l'analyse de départ.

## Par boutique

| Boutique | Liste des jeux possédés | Installer | Suivre la progression | Sans launcher ? |
|---|---|---|---|---|
| **Steam** | API Web Steam `IPlayerService/GetOwnedGames` : une clé d'API gratuite (steamcommunity.com/dev/apikey) + le SteamID64, lu localement dans `config/loginusers.vdf`. Avec sa propre clé, ça marche même si le profil est privé. | `steam://install/<appid>` : le client Steam (lancé en arrière-plan avec `-silent` s'il ne tourne pas) ouvre seulement sa petite fenêtre « Installer » (dossier, raccourcis), puis télécharge en arrière-plan. | En local : `steamapps/appmanifest_<appid>.acf` apparaît dès le début, avec `BytesToDownload` / `BytesDownloaded` et `StateFlags`. Canapé les lit déjà pour la bibliothèque. | **Presque** : le client Steam doit tourner (il le faut de toute façon pour jouer), mais sa grande interface ne s'ouvre pas. Une seule fenêtre de confirmation, utilisable à la manette. |
| **Epic** | [Legendary](https://github.com/legendary-gl/legendary) (`legendary list --json`). | `legendary install <app>` : téléchargement et installation sans le lanceur Epic. | Sortie de Legendary (progression en continu). | **Oui, complètement** : Legendary peut aussi lancer les jeux (`legendary launch`) et synchroniser les sauvegardes cloud. Connexion une fois par une fenêtre de login Epic (exe Windows ≥ 0.20.14), ou import de la session du lanceur Epic (`--import`). |
| **GOG** | API GOG (`embed.gog.com/account/getFilteredProducts`, ou `embed.gog.com/user/data/games` pour les seuls identifiants) avec un jeton OAuth. | Deux voies : télécharger l'installateur hors ligne du jeu et le lancer en silencieux (`/VERYSILENT`), ou [heroic-gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl), le module de téléchargement de Heroic. | gogdl : sa sortie ; installateur : la taille du fichier téléchargé. | **Oui**, mais plus de travail : gogdl n'est pas fait pour être utilisé directement (« not user friendly cli »), et l'auth GOG est à intégrer nous-mêmes. |
| **Xbox / Game Pass** | Pas d'API publique pour les jeux possédés ou le catalogue Game Pass PC. | `ms-windows-store://pdp/?productid=<id>` ou l'app Xbox : ouvre la fiche, l'installation passe par Gaming Services. `winget --source msstore` installe des apps du Store, pas les jeux Game Pass. | Non exposée. | **Non** : au mieux, ouvrir la fiche du jeu dans l'app Xbox. |

## Contraintes

- **Licences** : Legendary et heroic-gogdl sont sous **GPL-3**. Canapé (MIT) peut les **appeler comme programmes séparés** sans changer de licence. Les embarquer dans notre installateur reste possible (« agrégat »), à condition de fournir leur code source et leur licence. Le plus simple : les télécharger à la demande depuis leurs releases officielles dans `%LOCALAPPDATA%\Canape\tools`.
- **Légèreté** : rien ne tourne en permanence. Legendary et gogdl sont des exe PyInstaller (~15–20 Mo chacun), lancés seulement pendant une installation. La clé Steam et les jetons restent sur l'appareil (`shell.json`, et les fichiers d'auth des outils).
- **Droits** : tout se fait dans la session de l'utilisateur, sans élévation. Le shell lance les processus, comme pour les jeux.
- **Disque** : choix du dossier d'installation (le premier dossier de bibliothèque Steam, `C:\Games\Epic`, …) et de l'espace libre à vérifier avant de lancer. Fait (voir [État](#état)).

## Proposition

La proposition de départ ; les étapes 1 à 3 sont codées (voir [État](#état)).

1. **Étape 1, Steam** (sans outil tiers) : un filtre **« À installer »** dans la bibliothèque, avec les jeux Steam possédés (clé d'API dans les Réglages), leurs jaquettes (déjà gérées par les jaquettes téléchargées), et **A** = installer. La progression vient des manifestes locaux ; le jeu bascule dans la bibliothèque à la fin.
2. **Étape 2, Epic** : Legendary téléchargé à la demande. Connexion une fois depuis Canapé (fenêtre Epic), liste des jeux possédés, installation, puis lancement direct sans le lanceur Epic.
3. **Étape 3, GOG** : installateurs hors ligne en silencieux, ou gogdl. Choix retenu : gogdl, pour la progression et pour ne pas dépendre d'un installateur Inno Setup ; l'authentification passe par gogdl lui-même (`auth --code`), Canapé n'a qu'à ouvrir la page de connexion et récupérer le code.
4. **Xbox** : un bouton qui ouvre la fiche dans l'app Xbox, sans plus.
