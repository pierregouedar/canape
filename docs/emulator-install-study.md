# Installation d'émulateurs en un clic : étude

État au 31 octobre 2026 : réflexion et plan, rien de codé. Les adresses de téléchargement et les noms de fichiers ci-dessous viennent de ma connaissance des projets, **ils ne sont pas vérifiés** : chaque point marqué « à vérifier » doit l'être sur le site du projet avant d'écrire du code.

## Ce que Pierre veut

Depuis Canapé, à la manette, installer un émulateur choisi dans une courte liste, sans passer par un navigateur ni un dossier à la main, puis le retrouver dans la bibliothèque. Le serveur de mouvement existe déjà pour les émulateurs qui lisent un gyroscope (protocole DSU).

## Principes

1. **Source officielle seulement.** On télécharge depuis la page de versions du projet lui-même (GitHub Releases ou le serveur de builds du projet), jamais depuis un site tiers. Si l'adresse change, l'installation échoue proprement et le dit.
2. **Aucun jeu, aucun BIOS, aucune clé.** Canapé installe l'émulateur, pas de contenu protégé. Les BIOS et les images de jeux sont à fournir par l'utilisateur ; l'interface le dit.
3. **Vérification avant d'exécuter.** HTTPS, taille plausible, empreinte SHA-256 quand le projet en publie une (GitHub la donne pour chaque fichier de version). Sans empreinte, l'installation demande une confirmation explicite.
4. **Dossier à part, sans droits administrateur.** `%LOCALAPPDATA%\Canape\emulators\<nom>\`, jamais `Program Files`. Le shell reste en `asInvoker`. Désinstaller = supprimer ce dossier (les sauvegardes et réglages de l'émulateur restent dans son dossier de données, prévenu avant).
5. **Pas d'écriture dans les réglages de l'émulateur** (même règle que pour Lossless Scaling), sauf demande explicite plus tard (par exemple pointer le serveur de mouvement).
6. **Accord de Pierre avant chaque installation sur l'Ally** pendant la mise au point.

## Candidats (à confirmer avec Pierre)

| Émulateur | Machines | Téléchargement | À vérifier |
|---|---|---|---|
| RetroArch | Beaucoup (cœurs) | Serveur de builds du projet, archive 7z : il faut un extracteur ou le programme d'installation | format de l'archive, besoin de télécharger les cœurs un par un |
| Dolphin | GameCube, Wii | Page de téléchargement du projet (versions de développement en 7z) | format, canal stable ou non |
| PCSX2 | PlayStation 2 | GitHub Releases (archive portable) | nom exact des fichiers, besoin d'un BIOS |
| DuckStation | PlayStation 1 | GitHub Releases | présence d'une version portable |
| PPSSPP | PSP | Page du projet, archive zip | format |
| RPCS3 | PlayStation 3 | GitHub Releases | exigences matérielles : l'Ally est limité |
| Cemu | Wii U | GitHub Releases | clés du système à fournir par l'utilisateur |
| Xenia | Xbox 360 | GitHub Releases | état de compatibilité sur l'Ally |

À ne **pas** proposer : les émulateurs Switch dont les projets ont été arrêtés ou dont la distribution est contestée. Le choix de la liste engage Pierre : il vaut mieux commencer par deux ou trois (par exemple RetroArch, Dolphin, PCSX2) puis étendre.

## Étapes de l'installation

1. **Catalogue** : un fichier `emulators.json` dans le dépôt (nom, machines, adresse de la page de versions, motif du nom de fichier, exécutable attendu, indice « BIOS requis », compatible mouvement). Pas de logique propre à un émulateur dans le code, sauf l'extraction.
2. **Recherche de la dernière version** : appel à l'API de versions du projet (même mécanisme que `online::Http`, avec `WinHttp`).
3. **Téléchargement** dans un dossier temporaire, avec progression (comme l'installation Steam), vérification de l'empreinte.
4. **Extraction** : le zip se lit avec une dépendance déjà courante ; le 7z demande soit une bibliothèque, soit de refuser les projets qui n'en publient pas d'autre. À trancher.
5. **Enregistrement** : l'exécutable est ajouté à la bibliothèque comme un autre jeu (`games.json`, source « Autres »), avec son nom, et marqué comme émulateur pour la page « Gérer les jeux ».
6. **Mise à jour** : une ligne « Mettre à jour » sur la fiche, qui refait les étapes 2 à 5. Pas de mise à jour automatique.
7. **Mouvement** : si l'émulateur sait lire un serveur DSU, la fiche rappelle d'activer le serveur de mouvement de Canapé (port 26760 par défaut) et de le déclarer dans l'émulateur. Aucune écriture dans ses réglages.

## Ce que ça demande côté interface

- Une entrée « Installer un émulateur » dans la page « Gérer les jeux » (touche X dans la vue Jeux), ou une étagère « Émulateurs » en bas de la bibliothèque.
- Une liste de cartes (nom, machines, taille, « BIOS à fournir »), A pour installer, avec la page de progression des installations de jeux.
- Textes traduits dans les douze langues, comme le reste.

## Risques

- **Les noms de fichiers changent** d'une version à l'autre : le motif de nom doit être tolérant et l'échec explicite.
- **Antivirus et SmartScreen** peuvent bloquer un exécutable téléchargé : l'extraction dans un dossier de l'utilisateur et le dire à l'écran.
- **Aspect juridique et réputation** : on ne distribue rien nous-mêmes, on lance un téléchargement depuis le projet. Aucun BIOS, aucune ROM.
- **Place disque** : prévenir de la taille avant de télécharger.

## Prochaine décision pour Pierre

Quels émulateurs dans la première liste, et acceptons-nous les archives 7z (dépendance supplémentaire) ou seulement les zip ? Ensuite : catalogue, téléchargement et extraction d'un seul émulateur de bout en bout (par exemple PCSX2 ou Dolphin selon le format), essayé sur l'Ally avec son accord, puis généralisation.
