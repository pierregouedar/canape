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

## À poursuivre

Vérifier sur cette version les parcours FSE, manette, bibliothèque et lancement, puissance et retour à Windows. Tester les fonctions récentes documentées comme non validées, notamment les installations Steam, Epic et GOG, le gyroscope, les palettes, l'éclairage, la visite et le changement de rendu. Les gestes physiques et l'apparence à l'écran nécessitent une observation de la console ; ne pas déduire leur réussite des seules réponses IPC.
