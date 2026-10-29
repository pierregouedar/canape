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

L'overlay Windows s'est ouvert au-dessus du shell avec Win+G. Le processus du widget était présent, mais son panneau et sa connexion au service n'ont pas été observés pendant ce passage : ils restent à vérifier.

## À poursuivre

Vérifier sur cette version les parcours FSE, manette, bibliothèque et lancement, puissance et retour à Windows. Tester les fonctions récentes documentées comme non validées, notamment les installations Steam, Epic et GOG, le gyroscope, les palettes, l'éclairage, la visite et le changement de rendu. Les gestes physiques et l'apparence à l'écran nécessitent une observation de la console ; ne pas déduire leur réussite des seules réponses IPC.
