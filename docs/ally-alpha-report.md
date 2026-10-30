# Campagne alpha sur la ROG Ally

Essais en cours le 28 octobre 2026 sur une ROG Ally RC71L sous Windows, depuis une session SSH. Le dépôt utilisé pour les tests automatisés est au commit `0f161d8`, avec la correction locale de collision des dossiers temporaires des tests GOG. Les tests automatisés ne remplacent pas les essais de l'interface et du matériel.

## Tests automatisés

- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` et `cargo check --target x86_64-pc-windows-msvc --all-targets` : réussis.
- La compilation release du shell logiciel, du shell GPU, du service et de `canape-ctl` réussit sur l'Ally.
- `cargo test --workspace --exclude canape-shell` : réussi, 274 tests. La bibliothèque, dont les 124 tests ont réussi, a ensuite été exécutée 30 fois avec 16 threads sans échec.
- `cargo test --workspace` : 361 tests réussis après enregistrement de l'identité du paquet pour le compte SSH de test, avec `CustomCapability.SCCD` dans le dossier externe des exécutables de test. Avant cet enregistrement, Windows refusait de lancer le binaire de test du shell avec l'erreur 15609. Ce résultat ne teste pas le démarrage FSE.
- La première exécution a révélé une collision entre deux tests GOG qui partageaient un répertoire temporaire et pouvaient le supprimer pendant l'autre test. Leurs répertoires ont été séparés.

## Service déjà installé

Le service, le shell et le widget étaient déjà en cours d'exécution. La page Réglages indique `4560169` pour le shell ; le commit des autres binaires installés n'est pas établi. Dans la session console, la touche Page suivante a parcouru Jeux → Performance → Matériel → Réglages ; les pages affichaient les mesures et le service connecté. La navigation par les boutons physiques n'a pas été vérifiée. Le service répond à `get_status`, `list_profiles`, `get_fan_curves` et `get_bench_report`. Il a rejeté un TDP de 999 W, une limite de 999 FPS, une charge de batterie de 0 % et une couleur invalide. Une couleur d'accent modifiée a été relue puis remise à sa valeur initiale. Ces résultats concernent seulement la version déjà installée.

## Installation du commit courant

Le 28 octobre, `install-dev.ps1` a été exécuté depuis la session console de `White` avec les binaires release construits sur l'Ally et le certificat de développement existant. Le script a terminé sans erreur et réenregistré le paquet d'identité. Après lancement par l'identité du paquet, le service est actif en session 0 et le shell en session 1 ; le hash du shell installé correspond au build et son journal confirme l'identité `Canape`. `get_status` répond et les réglages principaux antérieurs sont toujours présents. Les exécutables précédents et l'état du service ont été sauvegardés sur l'appareil. Ce lancement manuel ne prouve pas encore le démarrage automatique du FSE après redémarrage.

Depuis `canape-ctl`, une limite manuelle de 15 W a été acceptée et affichée dans l'état ; le mode Turbo initial a ensuite été rétabli. Une limite de charge de 80 % a été acceptée et affichée, puis remise à 100 %. Les quatre commandes sont journalisées `Ok`. Ces lectures confirment les commandes et l'état déclaré par le service ; elles ne mesurent pas indépendamment la limite de puissance ni l'arrêt effectif de la charge.

De même, le service a accepté le plafond de 40 FPS puis sa suppression ; le préréglage de ventilation Équilibré puis le retour aux courbes automatiques ; l'affectation de M1 à A puis sa remise par défaut ; et l'activation « toujours » du gyroscope puis son arrêt. L'état et le journal ont confirmé ces changements et le retour aux réglages initiaux. La courbe personnalisée CPU/GPU figurait dans la réponse, puis était absente après réinitialisation. Aucun jeu n'était au premier plan : l'effet sur les FPS, la réponse physique de M1 et le mouvement au gyroscope restent à vérifier.

### Lancement Steam et limite de FPS

Avec Steam déjà ouvert, la navigation clavier dans Récents a permis d'ouvrir la fiche d'Among Us, avec ses images et son bouton Jouer. Jouer a lancé `steam.exe -silent steam://rungameid/945360` ; le jeu s'est ouvert en session console sur son écran de conditions, sans que celles-ci soient acceptées. Le service a identifié `Among Us.exe` au premier plan. Sur cet écran, la limite de 40 FPS a ramené la mesure à 39,9 FPS ; après suppression de la limite, la mesure était d'environ 60 FPS. Le jeu a été fermé par Alt+F4 et le premier plan est revenu à `null`. Ce passage ne prouve pas qu'aucune fenêtre Steam transitoire n'est apparue, ni le lancement avec Steam fermé.

Steam a ensuite été arrêté proprement alors qu'aucun jeu ne tournait, puis le même bouton Jouer a été utilisé. Steam et le jeu ont démarré et le jeu a atteint son écran de conditions. Des captures prises toutes les deux secondes montrent toutefois une fenêtre Steam « Démarrage du jeu » par-dessus Canapé juste avant l'apparition du jeu. Le démarrage n'est donc pas entièrement sans fenêtre dans ce cas. Le jeu a été fermé sans accepter ses conditions et Steam est resté ouvert, comme avant l'essai. L'intervalle entre captures ne permet pas de mesurer précisément la durée de la fenêtre.

Un troisième lancement, avec Steam déjà ouvert, a relevé le processus de la fenêtre au premier plan toutes les 250 ms pendant 20 s. Après une mesure sur Canapé, `steamwebhelper` apparaît pendant dix mesures successives, puis Among Us passe devant. Le relevé identifie le processus et non le contenu de la fenêtre ; il montre néanmoins que Steam prend aussi le premier plan dans ce cas. Le jeu a ensuite été fermé sans accepter ses conditions.

Le journal du shell montre ensuite, le 28 octobre, deux validations à la manette sur Toribash : Canapé a mis `steam:248570` en file, redémarré Steam pour relire le manifeste, puis enregistré la prise en charge du téléchargement et l'installation terminée 38 secondes après la demande. La bibliothèque est passée à 18 jeux. Ce constat vient du journal d'une interaction à la manette, sans capture continue : il ne prouve pas l'absence d'une fenêtre Steam ni le lancement du jeu installé.

Le 29 octobre, le shell a été relancé en session console avec l'argument `windows.gaming:///home`, et le service était déjà actif. Le dernier démarrage Windows enregistré date du 28 octobre : cela valide une activation FSE lors de la session du 29, pas un démarrage automatique après redémarrage de Windows.

### Auto TDP sous charge graphique

Heaven a été démarré directement en DirectX 11 et reconnu au premier plan. Au démarrage en Turbo, le service a relevé jusqu'à 84 °C ; la limite a été ramenée à 15 W avant l'essai. Avec l'auto TDP activé, cible 60 FPS et plage 7–25 W, 31 mesures consécutives indiquent 13–14 W de limite, 57,4–66,3 FPS (62,6 en moyenne) et 52–66 °C. La caméra n'a pas été figée : ce court passage confirme l'action du contrôleur sous charge, sans constituer un essai de stabilité ni une comparaison reproductible. Après l'essai, l'auto TDP a été désactivé, Heaven fermé, puis le mode Turbo initial rétabli. Alt+F4 n'ayant pas arrêté Heaven dans cette session, le processus lancé pour l'essai a été arrêté explicitement.

### Réglages Windows

Sur le plan d'alimentation actif, la préférence énergétique du processeur était à 0 sur secteur et batterie. Le service l'a passée à 80 puis remise à 0 ; `powercfg /qh` a confirmé indépendamment les deux valeurs sur les deux sources. Les autres réglages processeur ont été conservés.

Pour l'optimisation d'enregistrement en arrière-plan, `GameDVR_Enabled` valait 1 et `AppCaptureEnabled` était absent dans le registre de l'utilisateur console. L'activation a mis les deux à 0 ; la désactivation a remis la première à 1 et supprimé la seconde. Cette vérification porte sur l'écriture et la restitution du registre, pas sur l'enregistrement ni sur l'ouverture de la Game Bar.

### Changement de rendu

Le 29 octobre, depuis Réglages → Système → Affichage, le passage d'Économe à Fluide au clavier a redémarré le shell GPU dans la session console. Le journal confirme le rendu OpenGL, l'identité du paquet et la première image ; seul `canape-shell-gpu` était présent. Le retour à Économe a redémarré le shell logiciel, et le réglage enregistré est revenu à `software`. Mesures ponctuelles après démarrage : environ 238–248 Mo privés pour le GPU et 21 Mo pour le logiciel ; elles ne sont pas une comparaison stabilisée en jeu. Une erreur de navigation pendant l'essai a brièvement mis la limite de charge à 80 % ; elle a été remise à 100 % avant la fin.

Le 29 octobre, l'overlay Windows s'est ouvert au-dessus du shell avec Win+G et le panneau Canapé a été ouvert depuis le menu des widgets. Au repos, il affichait les mesures du service, leur graphique et le mode Turbo, sans jeu au premier plan. Un clic automatisé sur Performance puis sur Turbo a changé la sélection affichée ; le journal du service confirme `SetPerformanceMode` pour chaque mode avec une réponse `Ok`. Turbo a ainsi été rétabli. Le paquet installé indique la version `0.1.0.0` ; le commit de ce widget n'est pas établi. Les autres commandes et la navigation physique restent à vérifier.

Au repos, les commandes du widget ont aussi activé l'Auto TDP avec une cible de 60 FPS, augmenté cette cible à 90 FPS, puis l'ont ramenée à 60 FPS. Le journal confirme les trois requêtes `SetAutoTdp -> Ok` en conservant la plage 7–25 W. La sélection de Turbo a été acceptée ensuite ; la lecture de l'état confirme Turbo, Auto TDP désactivé et cible à 60 FPS, comme avant l'essai. Cet essai ne mesure pas la régulation en jeu.

Lors d'un lancement direct d'Among Us par Steam, le widget s'est également ouvert au-dessus de l'écran des conditions : il identifiait Among Us et affichait 60 FPS, 9 W et 42 °C au moment de la capture. Le jeu n'a pas dépassé cet écran et ses conditions n'ont pas été acceptées. Une première tentative distincte de commande en jeu n'a pas produit d'entrée correspondante dans le journal ; le processus du jeu a été arrêté après cet essai. Un second lancement suivi d'une séquence continue d'ouverture du widget et de clics sur Performance puis Turbo a enregistré les deux commandes `SetPerformanceMode -> Ok` pendant que le service détectait Among Us. Le jeu a ensuite été fermé par Alt+F4, le premier plan est revenu à `null` et Steam est resté ouvert. Ce test ne mesure pas indépendamment l'effet des modes sur la puissance et ne couvre pas une partie en cours.

La liste des profils étant initialement vide, le bouton d'enregistrement de l'onglet Puissance du widget a été utilisé sur l'écran des conditions d'Among Us. Le service a accepté `SaveForegroundProfile` et la liste a montré un profil `Among Us.exe` contenant notamment l'Auto TDP désactivé et la cible de 60 FPS. Au lancement suivant, l'état indiquait ce même exécutable comme profil actif. Le profil de test a ensuite été supprimé via `canape-ctl` et la liste était de nouveau vide ; le processus du jeu a été arrêté. L'application d'une valeur de profil différente des réglages courants n'a pas été mesurée.

## Essais du 29 octobre sur `31d2f86`

`main` a été remis sur `origin/main` (`31d2f86`) puis installé avec `install-dev.ps1` depuis la session console : le service a été réinstallé et redémarré, le paquet d'identité réenregistré. Le widget n'a pas changé depuis sa dernière compilation sur l'Ally et n'a pas été recompilé. Le shell a été relancé par l'identité du paquet.

### Boutons physiques

Pierre a pressé les boutons de l'Ally, Canapé au premier plan sur la page Jeux, pendant que l'écran était capturé toutes les 1,5 s et le journal du shell relu.

- Croix directionnelle et joystick gauche : la sélection change de jaquette dans Récents et descend sur Installés. Observé à l'écran.
- A ouvre la fiche du jeu sélectionné, B revient à la page Jeux. Le journal enregistre `keys: pad Return` puis `keys: pad Escape`.
- LB et RB : aucune capture de ce premier essai ne montre de changement de page, mais Pierre confirme ensuite qu'ils changent bien de page ; pendant la visite, Canapé est passé de Jeux à Réglages par Performance et Matériel. Le journal n'enregistre pas ces touches (seuls A et B y figurent).
- Le bouton à gauche de l'écran (Command Center) a ouvert la Game Bar Windows par-dessus Canapé, comme prévu. Pierre n'a pas signalé d'effet visible pour le bouton de droite (Armoury) ; Canapé était déjà sur Jeux, où ce bouton ne change rien. Son effet depuis un jeu reste à vérifier.
- Depuis ce lancement, `buttons: not supported` n'apparaît plus dans le journal, alors qu'il suivait chaque démarrage précédent de 5 s environ.

### Éclairage

Avec l'accord de Pierre, cinq états ont été envoyés par `canape-ctl`, à 8 s d'intervalle : rouge fixe au niveau 3, vert fixe au niveau 1, bleu en respiration rapide au niveau 3, niveau 0, puis éclairage rendu à la manette (`lighting: null`). Le service a répondu `Ok` à chaque requête et les a journalisées. Pierre, qui regardait les anneaux, confirme les cinq états dans l'ordre : rouge, vert faible, bleu qui pulse, éteint, puis l'éclairage habituel de la manette. Les réglages de l'interface (page Matériel, éclairage qui suit l'accent) n'ont pas été utilisés pour cet essai.

### Visite de bienvenue

Pierre a ouvert la visite à la manette depuis Réglages → Système, l'écran étant capturé toutes les 1,5 s. Pour s'y rendre, Canapé est passé à l'écran par Performance, Matériel puis Réglages ; la touche utilisée pour changer de page n'a pas été relevée. Le journal enregistre `onboarding: shown again from Settings`, cinq validations A, puis `onboarding: done` 15 s plus tard, et Canapé revient sur Jeux. Les captures montrent au passage les étapes Langue et Dossier des jeux ; les autres ont été franchies entre deux captures. Pierre n'a appliqué aucun réglage ni saisi aucune clé, et n'a constaté aucun problème. La lisibilité de chaque étape n'a pas été examinée image par image.

### Fréquence d'écran par profil

Avec l'accord de Pierre, un profil `Among Us.exe` avec `refresh_hz` à 60 a été créé par `canape-ctl`, l'écran étant à 120 Hz. Among Us a été lancé par `steam://rungameid/945360` (Steam déjà ouvert), et `Win32_VideoController.CurrentRefreshRate` relevé chaque seconde avec le premier plan déclaré par le service. Dès que le service a vu `Among Us.exe` au premier plan, environ 5 s après le lancement, l'écran est passé à 60 Hz et y est resté pendant les 50 s suivantes ; le shell a journalisé `screen: 60 Hz`. Le jeu a été fermé sur son écran de conditions, sans les accepter : moins de 2 s plus tard, le premier plan était vide et l'écran revenu à 120 Hz (`screen: 120 Hz`). Le profil de test a ensuite été supprimé. Le profil n'a pas été créé depuis l'interface et le jeu n'a pas été lancé depuis Canapé pour cet essai.

### Jeu Epic : installation, lancement, désinstallation

Avec l'accord de Pierre, Cave Story+ (0,2 Go, compte Epic déjà connecté) a été choisi dans l'étagère Epic. Sa fiche affichait « Non installé · 0,2 Go · 252,5 Go libres dans C:\Games ». Canapé a été piloté par des touches clavier envoyées à sa fenêtre, l'écran étant capturé toutes les 1,5 s.

- **Installation :** A sur Installer. La fiche a affiché « En attente de la boutique… », puis la barre de progression jusqu'à 100 %, et le bouton est devenu Jouer. Le journal enregistre l'installation terminée 12 s après la demande ; le jeu est dans `C:\Games\CaveStoryPlus` et la bibliothèque passe à 19 jeux.
- **Lancement :** A sur Jouer. Canapé lance le jeu par Legendary ; l'écran titre de Cave Story s'affiche 4 s plus tard, le service voit `CaveStory+.exe` au premier plan à 60 FPS. Le lanceur Epic était déjà ouvert depuis 20:08, avant l'essai : ce test ne montre donc pas un lancement sans lui. Aucune fenêtre du lanceur n'est apparue par-dessus le jeu sur la capture.
- **Désinstallation :** le jeu fermé, Y sur la fiche ouvre les options (Renommer, Catégories, Retirer de la bibliothèque, Désinstaller). Désinstaller demande une confirmation, puis le journal enregistre `Game has been uninstalled` en 2 s ; Canapé revient sur Jeux et la bibliothèque repasse à 18 jeux. Le dossier ne contient plus que la sauvegarde `Profile.dat` (0,1 Mo).

### Jeu GOG

Non testé : aucun compte GOG n'est connecté sur l'Ally (`owned.json` ne contient aucun jeu GOG, pas de jeton gogdl). La connexion demande que Pierre se connecte lui-même dans le navigateur. GOG est mis en pause : aucun essai prévu pour l'instant.

### Palettes M1 et M2

Avec l'accord de Pierre, M1 a été affectée à RB et M2 à LB par `canape-ctl` (état relu : `pad_rb`, `pad_lb`). Canapé au premier plan, Pierre a appuyé trois fois sur chaque palette : M1 avance d'une page, M2 recule d'une page, selon son observation. Les palettes ont ensuite été remises par défaut (`ResetBackButtons`, état relu `default`).

### Gyroscope

Among Us a été lancé par Steam ; le service l'a vu au premier plan 5 s plus tard. La visée au gyroscope a alors été activée en permanence (sensibilité 10, lacet horizontal) pendant 45 s, et la position du curseur relevée toutes les 0,5 s. Pierre a tourné et incliné l'Ally pendant les premières secondes : le curseur s'est déplacé sur toute la fenêtre pendant environ 8 s, puis il est resté immobile quand Pierre a cessé de bouger l'appareil, sans dérive pendant 36 s. Pierre décrit le mouvement comme fluide et contrôlable. La visée a ensuite été désactivée et le jeu fermé sans accepter ses conditions. Les modes « tant qu'une palette est tenue » et « bascule » n'ont pas été essayés.

### Démarrage de Windows

Pas de redémarrage pendant cette session. Pierre confirme, d'après son usage de l'Ally, que Windows démarre directement sur Canapé dans l'écran Xbox plein écran. Ce point n'a pas été observé ni journalisé ici. Le service `CanapeService` est en démarrage automatique et en cours d'exécution.

## Campagne 2 sur `238bb0a`

`main` remis sur `origin/main` (`238bb0a`, GOG retiré de l'interface) et installé avec `install-dev.ps1` ; Réglages affiche `0.1.0 (238bb0a)`. État relevé avant la campagne : mode Turbo, réglages processeur personnalisés (boost agressif, préférence 0, 2000 MHz), éclairage arc-en-ciel, serveur de mouvement activé, `shell.json` sauvegardé hors dépôt.

### GOG retiré de l'interface

- Réglages → Comptes : trois cartes, Steam, Epic Games, Durée de jeu ; droite s'arrête sur la troisième. Observé à l'écran.
- Visite, étape Boutiques : les lignes sont Clé Steam Web API, Compte Epic Games, puis Client ID et Client secret IGDB ; aucune ligne GOG. La navigation haut/bas d'Epic vers IGDB n'a pas été vérifiée séparément.
- Étagère À installer vide : non vérifiée, l'étagère contient des jeux sur cet appareil.

**Problème : une clé enregistrée vide efface la clé existante.** Dans l'étape Boutiques, A sur « Clé Steam Web API » ouvre un champ vide (et le clavier tactile Windows) ; un second A enregistre ce champ vide (`onboarding: steam saved`) et la clé Steam déjà enregistrée est remplacée par une chaîne vide, la ligne affichant ensuite « Non définie ». C'est arrivé pendant cet essai, par un appui de trop ; la clé a été remise depuis la sauvegarde de `shell.json`. Un enregistrement vide ne devrait pas effacer une clé existante, ou demander confirmation. Après l'arrêt du shell, le clavier tactile Windows est aussi resté ouvert par-dessus Canapé jusqu'à sa fermeture manuelle. Pour le reproduire : dans la visite, étape Boutiques, A sur « Clé Steam Web API » (le clavier tactile s'ouvre), puis arrêter le processus du shell et le relancer ; le clavier reste affiché par-dessus la page Jeux.

Revérifié sur `235adb1` : dans la visite, A sur « Clé Steam Web API » ouvre le champ vide, A l'enregistre ; la ligne affiche toujours la clé masquée (quatre points et les quatre derniers caractères) et la longueur de la clé enregistrée est inchangée. Le clavier tactile reste affiché après cet enregistrement, jusqu'à B. Rejouer la visite depuis Réglages remet `onboarded` à false dans `shell.json` dès qu'on y avance ; la visite a été quittée en arrêtant le shell et le drapeau remis à true.

### T1 Pleine puissance dans Canapé et animations allégées

Mode Silencieux réglé par `canape-ctl`, ventilateurs sur Équilibré (courbe personnalisée CPU et GPU dans l'état). CPU du shell mesuré par son temps processeur pendant 15 s de navigation rapide (flèches, haut/bas, LB/RB toutes les 150 ms sur la page Jeux), puis 10 s au repos. Les mesures sont en pourcentage d'un cœur ; la fréquence du processeur change selon le mode, elles ne mesurent donc pas les images affichées.

| Situation | Navigation | Repos |
|---|---|---|
| Silencieux, pleine puissance désactivée, Économe | 27,8 % | 3,0 % |
| Silencieux, pleine puissance activée, Économe | 85,0 % | 4,4 % |
| Limite manuelle 12 W, pleine puissance désactivée | 29,2 % | 1,9 % |
| Limite manuelle 15 W, pleine puissance désactivée | 36,8 % | 5,2 % |
| Silencieux, pleine puissance désactivée, Fluide (shell GPU) | 66,3 % | 3,4 % |

- Réglages → Système → Pleine puissance → Activé (A) : le service journalise `SetUiPower { enabled: true }` puis `ui power: on (shell in front, was silent)` ; l'état passe à `ui_power: true`, le mode enregistré reste Silencieux et la courbe Équilibré reste dans l'état.
- Among Us lancé par Steam : dès que le shell perd le premier plan, `ui power: off (shell left the foreground, back to silent)` ; jeu détecté, `ui_power: false`, mode Silencieux, courbe gardée. Jeu fermé et Canapé ramené devant : `ui power: on (shell in front, was silent)`.
- La puissance réellement écrite dans le firmware n'est pas relisible sur ce modèle : l'effet est établi par le journal et l'état du service, pas par une mesure indépendante.
- Fluidité : non jugée par Pierre pendant cet essai. L'écart de CPU entre pleine puissance désactivée et activée (28 % contre 85 %) est cohérent avec des animations coupées en Silencieux ; entre 12 W et 15 W l'écart est faible (29 % contre 37 %).
- Affichage → Fluide : le shell redémarre en `canape-shell-gpu` (`renderer: restarting as femtovg (OpenGL)`), puis Économe le ramène au shell logiciel ; `renderer` revient à `software`.
- **Problème : le serveur de mouvement ne reprend pas son port après un changement de rendu.** Activé dans les réglages, il journalise au redémarrage du shell `motion server: port 26760: … (os error 10048)` (port encore tenu par l'ancien processus) et n'écoute plus ensuite, ni en GPU ni après le retour au rendu logiciel.

État remis : Turbo, ventilateurs automatiques, pleine puissance désactivée, Économe.

### T2 Étagère Processeur

Page Performance, étagère Processeur, chaque tiroir est un curseur appliqué par gauche/droite. Après chaque cran, la valeur du plan actif (« Utilisation normale ») a été relue avec `powercfg /qh` (secteur / batterie), en plus de l'état du service.

| Réglage | Valeurs essayées → `powercfg` | Par défaut → `powercfg` |
|---|---|---|
| Boost (`PERFBOOSTMODE`) | Activé → 1/1, Désactivé → 0/0 | 2/2 |
| Préférence énergétique (`PERFEPP`) | 25 % → 25/25, 50 % → 50/50, 0 % → 0/0 | 25/60 |
| Fréquence maximale (`PROCFREQMAX`) | 2,8 GHz → 2800/2800, 3,6 GHz → 3600/3600, 2,0 GHz → 2000/2000 | 0/4000 |
| Cœurs actifs (`CPMINCORES`) | 25 % → 25/25, 50 % → 50/50 | 100/25 |

Les valeurs « Par défaut » sont celles que le service avait relevées avant son premier changement (boost 2/2, préférence 25/60, fréquence 0/4000) et, pour les cœurs, la valeur de départ du plan. Chaque changement est journalisé `SetCpuPower … -> Ok`. Les libellés du curseur sont tronqués (« Par déf… », « Désacti… »). Réglages de départ remis ensuite (boost agressif, préférence 0, 2000 MHz, cœurs par défaut) et revérifiés : 2/2, 0/0, 2000/2000, 100/25.

### T3 Optimisations Windows (partiel)

État de départ relevé dans le registre, Defender et `Get-Service` (pas seulement `list_tweaks`) : 13 réglages déjà appliqués par Canapé et réellement en place (Copilot, recherche web, suggestions, astuces, télémétrie, applications en arrière-plan, mode jeu, planification GPU, fenêtré optimisé, exclusions Defender `C:\Games` et `steamapps\common`, temps réel Defender coupé, intégrité de la mémoire coupée). Widgets marqué « par Canapé » mais absent du registre ; Game DVR non appliqué ; services ASUS tous démarrés (non touchés).

- **Recommandations** propose 2 réglages : Widgets et Enregistrement des jeux en arrière-plan. Après le second A (« Appuyez encore sur A »), le service répond `writing SOFTWARE\Policies\Microsoft\Dsh\AllowNewsAndInterests: error 5`, affiché en bas de page. La clé `Dsh` donne le contrôle total à Système et Administrateurs, mais même `reg add` lancé en administrateur est refusé : Windows protège cette valeur (le pilote UCPD tourne sur l'Ally). Widgets ne peut donc pas être appliqué ainsi.
- Seule la requête Widgets apparaît dans le journal du service : Game DVR, second élément de la sélection, n'a pas été envoyé ou pas traité.
- Ensuite la page reste bloquée : le compteur garde « 2 à appliquer », A sur **Rétablir** est reçu (journal `keys`) mais rien ne s'ouvre. Cause dans `tweaks.rs` : `busy` reste à `recommended` tant que `recommended_left` n'est pas vide, et un échec le laisse ainsi jusqu'au redémarrage du shell.
- Game DVR appliqué seul (`set_tweak`) : `GameDVR_Enabled=0`, `AppCaptureEnabled=0` relus, puis retiré.
- **Rétablir** n'a pas pu être essayé (page bloquée, puis arrêt de la campagne). Non essayé : les 14 réglages rétablis puis réappliqués, la liste Applications.
- État final comparé à l'état de départ : aucune différence.

### Vérification de `2ef4cb4`

Réinstallé depuis `main` (`67533cd`, `2ef4cb4`).

- **Libellés des curseurs Processeur** : « Par défaut », « Désactivé », « Activé », « Agressif », « 25 % »… entiers et lisibles.
- **Serveur de mouvement au changement de rendu** : Économe → Fluide et Fluide → Économe, chaque fois un seul message `os error 10048` puis `listening on 127.0.0.1:26760` 2 s après. Corrigé.
- **Visite rejouée puis quittée à mi-parcours** (shell arrêté à l'étape 7) : `onboarded` reste `true` pendant et après, la visite ne revient pas au relancement. Corrigé. La clé Steam garde ses 32 caractères.
- **Clavier tactile** : **toujours ouvert** après B et après A/A sur le champ de la clé Steam (5 s d'attente, captures). Aucun journal de masquage. Cause relevée : sur l'Ally, `IPTip_Main_Window` existe avec un style invisible et désactivé, alors que le clavier affiché est une fenêtre XAML de `TabTip.exe` ; `visible()` répond donc `Some(false)` et `hide()` ne fait rien. La fenêtre de l'hôte de saisie a un titre traduit (« Expérience d'entrée Windows »), la recherche par « Microsoft Text Input Application » ne la trouve pas. Effet de bord : la réouverture d'un champ avec le clavier déjà affiché le referme (bascule).

### Vérification de `7c8b2fe` et suite de T3

Réinstallé depuis `main` (`ac8b0c2`, `62fbadb`, `7c8b2fe`).

- **Sélection recommandée** (Widgets + Game DVR) : les deux requêtes partent dans la même seconde ; Widgets répond encore `error 5` dans le journal, Game DVR `Ok`. La page affiche Widgets « Indisponible », « Tout est appliq… » (libellé tronqué) et Rétablir passe de 14 à 15, sans bandeau d'erreur. Registre relu : `GameDVR_Enabled=0`, `AppCaptureEnabled=0`, Copilot toujours appliqué. Capture prise 0,3 s après le second A : déjà l'état final, on ne peut donc pas voir à l'œil un déblocage avant la fin (les réponses arrivent trop vite).
- **Rétablir** s'ouvre (liste des 15, les trois réglages de sécurité en orange, avertissement « Les applications supprimées ne sont pas réinstallées »). Après le double A : 15 requêtes `on: false` `Ok`, compteur à 0, « 11 à appliquer ». Relecture réelle : toutes les valeurs d'avant Canapé sont revenues (Copilot, recherche, suggestions `SystemPaneSuggestionsEnabled=1`, télémétrie, mode jeu, GPU, Game DVR `1`, exclusions Defender vides, temps réel Defender réactivé, intégrité de la mémoire à sa valeur d'origine 0).
- **Réapplication** par Recommandations : 11 réglages, tous `Ok` (dont les exclusions Defender, signalées en orange). Les applications en arrière-plan, le temps réel Defender et l'intégrité de la mémoire ne font pas partie des recommandations : remis par `set_tweak` pour retrouver l'état de départ, Game DVR retiré. **Diff avec l'état de départ : vide.** Widgets n'est plus marqué « par Canapé ».
- **Applications** : 16 lignes ; 15 marquées « Réinstaller » (déjà retirées : Clipchamp, Actualités, Météo, Bing Search, Obtenir de l'aide, Hub de commentaires, Power Automate, Pense-bêtes, Mobile connecté, Family, Assistance rapide, Dev Home, Enregistreur audio, Calculatrice, Photos) et Microsoft 365 « Retirer ». Concorde avec `Get-AppxPackage`. Rien n'a été retiré ni réinstallé.
- **Clavier tactile** : dans la visite, champ de la clé Steam : ouvert à A, **fermé** après B, rouvert, **fermé** après A/A (captures). La clé garde ses 32 caractères. Corrigé.
- **Étape Dossier des jeux** : « Là où s'installent les jeux Epic. Steam garde ses propres dossiers. » Plus de GOG. Visite quittée à l'étape 5, `onboarded` reste `true`.

### T4 Limite de charge (non concluant)

Page Matériel, étagère Batterie, tiroir « Limite de charge » (60 %, 80 %, 100 %). 60 % appliqué : `SetBatteryChargeLimit { percent: 60 } -> Ok`, carte « 60 % », `battery_charge_limit=60`. Mais la console était branchée et **déjà à 100 %** : pendant 5 min (relevé toutes les 30 s), `Win32_Battery` reste à 100 %, `BatteryStatus=2`, et `root\wmi BatteryStatus` donne `Charging=False`, `Discharging=False`, débit 0, capacité 32 207 mWh constante. La batterie ne se décharge pas vers 60 % sur secteur, et l'arrêt de la charge ne peut pas se voir à 100 %. Remis à 100 % (`-> Ok`). À refaire batterie sous 60 % (il faut débrancher pour la vider, geste de Pierre).

### T5 Définition d'écran par profil et écran Image

Définition réelle relue par `Win32_VideoController` (toutes les 3 s).

- Among Us lancé depuis Canapé (l'écran des conditions d'utilisation du jeu reste affiché, non accepté). Retour dans Canapé : la page Matériel affiche « AMONG US », la carte Résolution devient active (désactivée sans jeu au premier plan). Tiroir : 1920×1080, 1600×900, 1280×720. 1280×720 + A : l'écran passe en **1280×720** tout de suite (`screen: 1280x720`). X « Enregistrer pour ce jeu » : `SaveProfile … resolution: Some(1280x720) -> Ok`, « Profil enregistré », en-tête « AMONG US · profil du jeu ».
- Jeu fermé : **1920×1080** en 3 s (`screen: 1920x1080`).
- Jeu relancé depuis Canapé : **1280×720** environ 4 s après le lancement, et toute la partie ; jeu fermé : retour à **1920×1080**. La fréquence reste 120 Hz.
- Écran Image : Mise à l'échelle → « AMD (RSR) » et Génération d'images → « AMD (AFMF) » affichent **« Pas encore appliqué »** à côté du titre du tiroir, la carte prend la valeur. X enregistre : `upscaling: AmdRsr`, puis `frame_gen: AmdAfmf` dans le profil. Remis à « Aucune » et réenregistré.
- Remarques : la fiche du jeu résume le profil par « Auto TDP désactivé » seulement, sans la définition. Le profil enregistre aussi le gyroscope et la fréquence courants.
- Incident : en relançant, la touche droite a été perdue et **Catherine Classic** a été lancé à la place d'Among Us (l'ordre des Récents avait changé). Fermé au bout d'une minute environ, aucune touche envoyée au jeu.

### T6 Options d'un jeu (Among Us)

Fiche du jeu, Y « Options » : Renommer, Catégories, Retirer de la bibliothèque, Désinstaller (non touché).

- **Renommer** : champ prérempli et sélectionné ; « Among Us Test » validé par A (`library: steam:945360 renamed`, `overrides.json` écrit). Titre changé sur la fiche et dans les étagères ; une ligne « Reprendre le titre de la boutique · Among Us » apparaît et remet le titre d'origine.
- **Catégories** : « Nouvelle catégorie » → « Essai » créée et cochée ; une étagère « Essai · 1 » apparaît sur la page Jeux, sous Installés. Décocher la retire, et la catégorie vide disparaît de la liste.
- **Retirer de la bibliothèque** : appliqué sans confirmation (`hidden`) ; Installés passe de 16 à 15, Steam de 63 à 62, l'étagère Essai disparaît. Le tiroir l'annonce : « Les jeux masqués se retrouvent dans Réglages › Ajouter un jeu ». Là, « Jeux masqués · 1 » → « Afficher à nouveau » (`shown again`), tout revient (Installés 16, Essai · 1). Il n'y a pas d'option « Masquer » distincte : Retirer joue ce rôle.
- **Clavier tactile : n'apparaît pas** dans le champ Renommer ni dans « Nouvelle catégorie » (aucune ligne `touch keyboard` dans le journal, captures sans clavier), alors qu'il s'ouvre bien dans les champs de clé de la visite. La saisie au clavier physique fonctionne.
- État final : titre d'origine, aucune catégorie, rien de masqué, `overrides.json` vide.
- Vu en passant : « art of rally Standard Edit… » tronqué en titre de la page Jeux ; Heaven Benchmark sans jaquette (titre sur fond brun).

### T7 Ajouter d'autres jeux

- **Par `games.json`** : entrée « Essai Charmap » (`C:\Windows\System32\charmap.exe`, la Table des caractères) ajoutée à la main. Le shell ne la voit qu'après un changement de page (`library: 17 games found`). Étagère « Autres jeux · 2 », carte sans jaquette (titre sur fond brun). A sur Jouer : `launching other:… charmap.exe`, la fenêtre « Table des caractères » s'ouvre devant Canapé. Fermée, entrée retirée, `games.json` remis.
- **Par l'interface** (Réglages › Bibliothèque › Ajouter un jeu) : la liste lit les raccourcis du menu Démarrer (Deluge `deluge.exe`, Heaven Benchmark 4.0, Tailscale `tailscale-ipn.exe`) ; une entrée déjà présente porte « Ajouté ». A sur Heaven **le retire tout de suite, sans confirmation** (`add games: removed`, la ligne affiche alors `heaven.bat`), A à nouveau le remet (`added`). Le retrait a été fait par erreur : la ligne « Jeux masqués » disparaît quand elle est vide, et la liste remonte d'un cran sous le curseur. `games.json` final identique à celui de départ.
- Le compteur « Installés » des Réglages reste à 17 après le retrait de l'entrée, jusqu'à la relecture suivante de la bibliothèque.

### T9 Éclairage et gyroscope par l'interface (partiel)

- **Éclairage** (Matériel › Manette › Éclairage des sticks) : Luminosité, Effet, Vitesse et 10 pastilles (la première « Suit la couleur d'accent »). Changer l'effet marque la valeur d'un point orange et propose « A Appliquer » ; les effets vus : Arc-en-ciel, Cycle de couleurs, Respiration, Fixe, Stroboscope. « Fixe » + A : `SetLighting … effect: Steady -> Ok`, carte « Fixe », Vitesse grisée, pastilles vives. Pastille verte + A : `color: Some("#10b981") -> Ok`. La couleur réelle des sticks n'a pas été regardée (personne devant la console). Remis en arc-en-ciel, niveau 3, rapide.
- **Gyroscope** (Visée au gyroscope › Actif) : Désactivé → « Tant qu'une palette arrière est tenue » (`hold`) → « Une palette arrière l'active et le coupe » (`toggle`) → « Toujours » (`always`) → Désactivé ; chaque cran est appliqué tout de suite par le service. Remis à Désactivé.
- **Non essayé** : l'effet réel dans Among Us, palette tenue ou basculée en bougeant la console. Il faut quelqu'un qui tient l'Ally.

### T10 Widget

Among Us lancé depuis Canapé, Game Bar ouverte (Win+G), widget Canapé ouvert à la souris.

- En-tête : « Among Us · profil du jeu », consommation, images/s, température, courbe. Onglet **Puissance** : modes, limite de puissance (« Fixé par le mode du firmware » en Turbo). Onglet **Matériel** : Ventilateurs (Auto, Équilibré, Maximum), Limite de FPS, Limite de charge, Manette (Manette, Clavier, Souris).
- **Limite de FPS** › : 30 FPS (`SetFpsLimit 30 -> Ok`), mais le jeu déjà lancé reste à environ 120 FPS. « Enregistrer pour ce jeu » : `SaveForegroundProfile { part: Hardware } -> Ok`, `fps_limit: 30` dans le profil. Le premier clic sur un bouton du widget ne fait que lui donner le focus ; aucune confirmation visible après l'enregistrement.
- Jeu fermé, la limite courante reste à 30 (elle n'est pas rendue à la sortie du jeu) ; remise à Aucune pour que le profil diffère des réglages. **Jeu relancé** : `fps_limit=30` dès le premier relevé, puis **30,0 FPS** stables pendant 30 s, et la définition du profil (1280×720) appliquée. La limite du pilote ne prend donc effet qu'au démarrage du jeu.
- **« Ouvrir Canapé ›»** (deuxième clic) : Canapé repasse devant, sur la fiche d'Among Us, qui résume maintenant le profil par « Auto TDP désactivé · limite 30 FPS » ; l'en-tête affiche 31 FPS.
- **Bouton Armoury depuis un jeu : non essayé**, il faut appuyer sur la touche de la console.
- Vu en passant : au retour de 720p à 1080p, le fond de l'accueil de Canapé ne couvre plus que les 1280 premiers pixels de large (capture), jusqu'au rafraîchissement suivant.
- Remis : profil Among Us supprimé (`profiles: []`, comme au départ), limite de FPS Aucune, 1920×1080.

### Vérification de `284703f` (clavier tactile des options d'un jeu)

Réinstallé depuis `main` (`d1a2be4`). Fiche d'Among Us › Options :

- **Renommer** : le clavier tactile s'ouvre avec le champ (`touch keyboard: shown`, capture) et se ferme à B ; nom inchangé.
- **Catégories › Nouvelle catégorie** : le clavier s'ouvre, se ferme à B ; rouvert puis A sur le champ vide : le clavier se ferme et aucune catégorie n'est créée.
- Windows affiche une fois sa propre bulle « Vous avez besoin d'un clavier tactile ? » au-dessus du clavier (non touchée).
- `overrides.json` reste vide. Corrigé.

### Vérification de `abcc7ee` (limite de FPS, fenêtre, widget)

Réinstallé depuis `main`, widget recompilé et réinstallé (`build-widget.ps1`, paquet du 30 octobre 21:51). Après cette réinstallation, l'icône Canapé avait disparu de la barre de la Game Bar : remise par le menu Widgets › Canapé.

- **Limite de FPS sans profil** : Among Us sans profil (pas de pastille « profil du jeu »), widget › Matériel › Limite de FPS › 30 (`SetFpsLimit 30 -> Ok`, état 30). Jeu fermé : `ForegroundApp None`, limite **Aucune**. Corrigé.
- **Fond après 720p** : profil Among Us en 1280×720 enregistré depuis Canapé (`SaveProfile … 1280x720`) ; au passage en 720p le journal note `window: 1920x1080 on a 1280x720 screen, full screen again`. Jeu fermé : `screen: 1920x1080`, et le fond de l'accueil couvre toute la largeur (capture). Corrigé.
- **« Enregistré » dans le widget : non vu.** Jeu au premier plan, un seul clic sur « Enregistrer pour ce jeu » suffit cette fois (`SaveForegroundProfile { part: Hardware } -> Ok`), mais le bouton garde « Enregistrer pour ce jeu » sur les captures prises 0,4 s, 1 s et 3 s après. Le service a bien reçu la demande ; c'est l'affichage du widget qui ne change pas (la réponse n'est peut-être pas reconnue par `SendAsync`).
- **Premier clic** : la fois précédente, le premier clic n'avait rien fait ; cette fois le widget venait d'être ouvert par un clic et un seul clic a suffi. Cela ressemble à la Game Bar qui active d'abord la fenêtre du widget, sans certitude.
- Vu en passant : l'horloge de la Game Bar reste figée (21:52 affiché à 21:55) alors que les valeurs du widget bougent.
- Remis : profil supprimé (`profiles: []`), limite de FPS Aucune, 1920×1080.

### T11 Visite de bienvenue complète (`374c067`)

Rejouée depuis Réglages › Système › Visite de bienvenue, toutes les étapes capturées, rien appliqué (X « Passer » / « Plus tard » partout, A seulement sur Démarrer, Langue inchangée et « Aller aux jeux »). Neuf étapes :

1. Bienvenue (« Service connecté »).
2. Langue : la liste montre maintenant **« 中文 »**, lisible ; aucun libellé coupé.
3. Couleur d'accent (10 pastilles, orange sélectionnée).
4. Boutiques de jeux : Steam (« 12 jeux installés », clé masquée), compte Epic, Durée de jeu (IGDB) Client ID et Client secret masqués. Bas : Clé Steam → Compte Epic → Client ID → Client secret (l'aide IGDB s'affiche et la liste remonte), haut revient jusqu'à Epic. Aucune clé saisie.
5. Dossier des jeux : « Là où s'installent les jeux Epic. Steam garde ses propres dossiers. » (plus de GOG), « C:\Games · 251,7 Go libres ».
6. Boutons de l'Ally (Armoury, Command Center, « Utiliser les boutons de l'Ally · Activé »).
7. Optimisations de Windows : « 2 à appliquer », bouton « Appliquer la sélection recommandée » non pressé. Le compte inclut Widgets, que Windows refuse (affiché « Indisponible » seulement après une tentative).
8. Affichage : Économe / Fluide, « Canapé redémarre à la fin de la visite ».
9. C'est prêt : rappel LB/RB, A, B, X, Y ; A « Aller aux jeux » → `onboarding: done`, retour à l'accueil.

`shell.json` comparé avant/après (clés comparées par longueur seulement) : seule différence `onboarding_step` 5 → 0 ; `onboarded` reste `true`, clés et autres réglages inchangés. Pas d'étape « clavier tactile » ni « presse-papiers » distincte : l'aide de l'étape Boutiques renvoie à A (saisir) et Y (coller). Le libellé « Tout appliqué » n'a pas été revu (rien n'a été appliqué).

### T12 Rendu Fluide en jeu et repli

Mesures par `Get-Process` (CPU rapporté à tous les cœurs, sur 10 à 15 s).

| Situation | CPU | Mémoire de travail | Privée |
|---|---|---|---|
| Économe (`canape-shell`), accueil, repos | 0,1 % | 53 Mo | 28 Mo |
| Fluide (`canape-shell-gpu`), accueil, repos | 0,1 % | 157 Mo | 200 Mo |
| Fluide, Among Us au premier plan (deux relevés) | 0,1 % | 183 Mo | 255 Mo |
| Fluide, retour après fermeture du jeu | 0,1 % | 182 Mo | 255 Mo |

- Passage à Fluide par Réglages › Affichage : un seul `os error 10048` puis `listening` 2 s après, comme vu plus haut. L'en-tête de Canapé en Fluide affiche « 2 FPS » à l'accueil (le shell lui-même est compté).
- Écart mesuré : environ 130 Mo de mémoire de travail et 230 Mo de privée en plus, conforme à l'avertissement « environ 300 Mo ». La fluidité ressentie en jeu n'a pas été jugée (personne devant la console).
- **Repli : bug.** Shell arrêté, `canape-shell-gpu.exe` renommé, Canapé relancé : le journal dit `gpu renderer: canape-shell-gpu.exe not found next to this executable; back to the software renderer` et `shell.json` repasse à `software`, mais **le processus s'arrête aussitôt, sans fenêtre** (aucun `canape-shell` en cours 15 s après, aucun événement d'erreur). Le lancement suivant ouvre bien Canapé en Économe. Piste : `hand_over_at_startup` (`renderer.rs:145`) quitte quand `instance::acquire()` échoue ; ici aucune autre instance n'était lancée, donc la reprise du verrou semble échouer à tort. L'interface n'annonce pas le repli.
- Remis : `canape-shell-gpu.exe` à sa place, Économe, shell en cours.

### T14 Désinstallation (`uninstall-dev.ps1`) et réinstallation

Fait avant T13 : le redémarrage coupe la session de test, et T13 vérifie ainsi aussi le démarrage après réinstallation. Sauvegarde préalable (hors dépôt) : `%LOCALAPPDATA%\Canape`, `state.json`, binaires de `C:\Program Files\Canape`, clé `GamingConfiguration`, état du service, relevé réel des optimisations Windows.

- Script lancé en administrateur, code 0. Il liste les 15 applications retirées par Canapé (à réinstaller depuis le Store si besoin), puis : `CanapeService removed`, paquets retirés, `GamingHomeApp removed (Canape_84zrj1c9kn010!App)`, dossier d'installation supprimé, données et certificat gardés.
- **Ce qui reste** : plus de service, plus de paquet `Canape` ni `Canape.Widget`, plus de `C:\Program Files\Canape`, aucun processus `canape*`. Restent `C:\ProgramData\Canape` (`state.json`, `service.log`), `%LOCALAPPDATA%\Canape` (14 fichiers, `shell.json` avec la clé Steam intacte) et le certificat `CN=Canape Dev`. `GamingHomeApp` est supprimée (`StartupToGamingHome` reste à 1, l'accueil plein écran revient donc à celui de Windows).
- **Rendu à Windows** : optimisations rétablies (Copilot, recherche, télémétrie… aux valeurs d'avant Canapé, temps réel Defender réactivé, exclusions vides), plan d'alimentation remis à ses valeurs (boost 2/2, préférence 25/60, fréquence 0/4000), éclairage rendu à la manette. Le script ne le dit pas pour le processeur, mais c'est relu par `powercfg`.
- **Réinstallation** `install-dev.ps1 -SetAsHomeApp` puis `build-widget.ps1` : codes 0, `GamingHomeApp = Canape_84zrj1c9kn010!App`, service démarré automatique, deux paquets présents, Canapé s'ouvre sur l'accueil (capture), `onboarded=true`, aucun profil, Turbo. Le service ne reprend pas de lui-même les optimisations ni l'étagère Processeur : remises par `set_tweak` (13 réglages) et `set_cpu_power` (agressif, 0 %, 2 000 MHz). Éclairage remis en arc-en-ciel. **Relevé final des optimisations identique à celui d'avant la désinstallation.**
- Vu en passant : `GameDVR_Enabled` valait 0 avant la désinstallation, alors que Canapé l'avait remis à 1 à 20:25 et que rien dans le journal du service ne l'a changé depuis (la Game Bar a été beaucoup utilisée pendant T10). Laissé tel quel.

### T13 Redémarrage de Windows

Redémarrage lancé le 30 octobre à 22:30:12 (`shutdown /r /t 60`), Canapé étant l'accueil plein écran (`GamingHomeApp = Canape_84zrj1c9kn010!App`, `StartupToGamingHome = 1`).

- Démarrage de Windows : 22:32:01 (`LastBootUpTime`).
- **Service lancé seul** : `service starting, device = ROG Ally` à 22:32:16, 15 s après le démarrage, avant l'ouverture de session ; réglages restaurés (Turbo, étagère Processeur, éclairage), canal pour le widget ouvert. `CanapeService` en cours, démarrage automatique.
- Ouverture de session : 22:33:34 (Winlogon, événement 7001).
- **Shell lancé seul par l'écran Xbox** : `canape-shell 0.1.0 started, args=["windows.gaming:///home"]` à 22:33:35, **1 s après l'ouverture de session**, en rendu logiciel, un seul processus ; bibliothèque lue en 34 ms, serveur de mouvement à l'écoute du premier coup.
- Capture : le relevé automatique (dossier Démarrage) n'a tourné qu'à 22:34:57, Windows retardant les programmes de démarrage ; Claude était alors devant, la capture ne montre donc pas Canapé. Pierre a relancé la session depuis l'Ally après le démarrage. L'élément de démarrage s'est supprimé comme prévu.
### Vérification de `bf3c85d` (repli, en-tête, Widgets verrouillé)

Réinstallé (`install-dev.ps1 -SetAsHomeApp`, `build-widget.ps1`).

- **Repli sans programme GPU** : Fluide choisi, `canape-shell-gpu.exe` renommé, Canapé relancé : `gpu renderer: canape-shell-gpu.exe not found…; back to the software renderer` puis le démarrage continue (langue, serveur de mouvement, bibliothèque) et **Canapé s'ouvre** en Économe (capture), `renderer=software`. Corrigé. Aucun message à l'écran n'annonce le repli. Programme remis.
- **En-tête en Fluide** : plus de « 2 FPS » à l'accueil sans jeu (capture). Corrigé.
- **Widgets verrouillé** : `set_tweak widgets_off` → `error 5`, `unavailable: locked` ; après `Restart-Service CanapeService`, toujours `locked`. La page affiche Widgets « Indisponible », Recommandations « 1 à appliquer » et le tiroir ne propose plus que l'enregistrement en arrière-plan : plus de nouvelle tentative. Rien appliqué ; état Windows identique au départ.

## Bilan de la campagne 2 (29 et 30 octobre)

**Validé sur l'Ally** : T1 pleine puissance et animations allégées ; T2 étagère Processeur ; T3 optimisations Windows (recommandations, Widgets verrouillé, Rétablir, réapplication, liste Applications) ; T5 définition par profil et écran Image ; T6 options d'un jeu (renommer, catégories, retirer et réafficher, clavier tactile) ; T7 autres jeux ; T10 widget (onglet Matériel, profil appliqué au lancement, « Ouvrir Canapé ») ; T11 visite complète ; T12 rendu Fluide (mémoire, CPU, repli) ; T13 redémarrage (service puis shell lancés seuls) ; T14 désinstallation et réinstallation ; et les correctifs vérifiés en cours de route (clé vide gardée, visite qui garde `onboarded`, serveur de mouvement qui reprend son port, clavier tactile des clés et des options, limite de FPS rendue après un jeu sans profil, fenêtre remise plein écran après 720p, « 中文 », GOG retiré).

**Reste à faire, gestes de Pierre** : T4 limite de charge avec la batterie sous 60 % ; T9 palette tenue ou basculée en jeu et couleur réelle des sticks ; touche Armoury depuis un jeu ; T8 clés IGDB, succès Steam, SteamGridDB ; « Profil enregistré » du widget à regarder à l'œil (les captures de la Game Bar ne se rafraîchissent pas : son horloge reste figée) ; fluidité ressentie de Fluide en jeu.

**Défauts encore ouverts** :
- La réinstallation après `uninstall-dev.ps1` ne réapplique ni les optimisations Windows ni l'étagère Processeur (à refaire à la main).
- Le repli vers le rendu logiciel n'est pas annoncé à l'écran.
- La limite de FPS du pilote n'agit qu'au lancement d'un jeu (sans effet sur un jeu déjà ouvert).
- Ajouter un jeu : A retire une entrée sans confirmation, et la liste remonte d'un cran quand « Jeux masqués » disparaît.
- La visite compte encore Widgets dans « 2 à appliquer » tant que Windows ne l'a pas refusé une fois.
- Les programmes du dossier Démarrage ne tournent qu'environ 80 s après le démarrage avec l'écran Xbox (constat Windows, utile pour les outils de test).
- Vus en passant : « art of rally Standard Edit… » tronqué, Heaven Benchmark sans jaquette.

**État laissé sur la machine** : Canapé `19b4c7e` installé, accueil plein écran Canapé, service en automatique, widget installé (à épingler de nouveau dans la Game Bar, menu Widgets) ; Turbo, étagère Processeur agressif / 0 % / 2 000 MHz, éclairage arc-en-ciel, gyroscope désactivé avec serveur de mouvement actif, limite de charge 100 %, aucune limite de FPS, aucun profil de jeu ; Économe, accent #f97316, `onboarded=true`, clé Steam intacte ; optimisations Windows comme au départ (13 appliquées par Canapé, Widgets verrouillé, services ASUS non touchés). Deux écarts venus de Windows et laissés tels quels : `GameDVR_Enabled` revenu à 1 (sa valeur d'origine) et `LightingService` (ASUS, démarrage manuel) arrêté depuis le redémarrage.
### Remarques en passant

- Visite, étape Langue : le bouton chinois affiche « 体中文 », le premier caractère de « 简体中文 » manque.
- Visite, étape Dossier des jeux : le texte parle encore de GOG (« les jeux Epic et GOG »).
- Visite, étape Optimisations : le chiffre de « 1 à appliquer » est dans une autre police.

## À poursuivre

Vérifier sur cette version les parcours FSE, manette, bibliothèque et lancement, puissance et retour à Windows. Tester les fonctions récentes documentées comme non validées, notamment les installations Steam, Epic et GOG, le gyroscope, les palettes, l'éclairage, la visite et le changement de rendu. Les gestes physiques et l'apparence à l'écran nécessitent une observation de la console ; ne pas déduire leur réussite des seules réponses IPC.
