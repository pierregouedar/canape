# Canapé : feuille de route

État au 29 octobre 2026.

À lire avant de commencer : `AGENTS.md` à la racine du dépôt `pierregouedar/canape` (branche `main`). Ses règles s'appliquent à chaque point.

Méthode : on développe, puis on teste simplement sur l'Ally. Sur l'Ally, demander l'accord de Pierre avant tout lancement de jeu, installation, écriture sur la manette ou redémarrage. Ne jamais affirmer qu'un comportement Windows marche sans l'avoir observé.

## T. À éprouver sur l'Ally (du plus utile au moins utile)

Ces points ont été vérifiés comme absents des rapports d'essais (`docs/ally-alpha-report.md`, `docs/m5-report*.md`, `docs/gpu-renderer-test.md`).

- [x] **1. Pleine puissance dans Canapé et animations allégées.** Éprouvé le 29 octobre (rapport, T1). Reste : la fluidité ressentie, à juger par Pierre.
- [x] **2. Étagère Processeur depuis l'interface.** Éprouvé le 29 octobre (T2), libellés corrigés et revérifiés.
- [ ] **3. Page Optimisations Windows refaite.** Partiel (T3 du rapport). À corriger d'abord : Widgets refusé par Windows (error 5 sur `Dsh\AllowNewsAndInterests`, même en administrateur) ; après un échec, la page reste bloquée (`busy` à `recommended`) et le reste de la sélection n'est pas appliqué. Puis éprouver Rétablir, la réapplication et la liste Applications.
- [ ] **4. Limite de charge de la batterie.** Le service accepte 80 %, mais l'arrêt réel de la charge n'est pas mesuré (console branchée sous 80 %).
- [ ] **5. Définition d'écran par profil et écran Image.** Seule la fréquence d'écran a été essayée. La résolution (720p) et les choix d'image ne sont pas vérifiés.
- [ ] **6. Options d'un jeu.** Renommer, catégories, masquer, retirer, clavier tactile. Le menu s'ouvre et Désinstaller marche ; le reste non.
- [ ] **7. Ajouter d'autres jeux** (`games.json`, raccourcis).
- [ ] **8. Durées IGDB, succès Steam, images SteamGridDB, icônes.** Nécessitent les clés de Pierre.
- [ ] **9. Éclairage par l'interface** (page Matériel) et **gyroscope avec palette** (modes maintenir et basculer).
- [ ] **10. Widget.** Onglet Matériel, application d'un profil différent des réglages courants, bouton Armoury depuis un jeu.
- [ ] **11. Visite de bienvenue : étapes non vues.** Optimisations, affichage, clés, clavier tactile, presse-papiers.
- [ ] **11 bis. Clavier tactile resté ouvert.** Toujours vrai sur `2ef4cb4` après A/A et après B : `visible()` lit `IPTip_Main_Window` (invisible) alors que le clavier affiché est la fenêtre XAML de `TabTip.exe` ; le titre de l'hôte de saisie est traduit. Détails dans le rapport.
- [ ] **11 quater. Corrigé le 29 octobre, à revérifier sur l'Ally :** Widgets refusé par Windows (le tweak devient « verrouillé », la page ne reste plus bloquée), clavier tactile non détecté (fermeture forcée quand Canapé l'a ouvert), texte du dossier des jeux sans GOG. Reste : « 体中文 » et la police du chiffre « 1 à appliquer ».
- [ ] **11 ter. Visite :** « 体中文 » au lieu de « 简体中文 » ; texte du dossier des jeux qui cite GOG ; chiffre de « 1 à appliquer » dans une autre police.
- [ ] **12. Rendu Fluide en jeu.** Fluidité, consommation et repli. Démarrage dans l'écran Xbox plein écran avec le shell GPU.
- [ ] **13. Redémarrage de Windows.** Démarrage automatique dans l'écran Xbox plein écran, observé et journalisé.
- [ ] **14. Script de désinstallation** (`uninstall-dev.ps1`).

Déjà éprouvé, inutile de refaire : boutons physiques et palettes M1/M2 ; éclairage par commandes ; fréquence d'écran par profil ; Epic (installation, lancement, désinstallation) ; gyroscope et serveur de mouvement ; auto TDP, banc d'essai, limite de FPS, ventilateurs, mode manette ; bascule Économe/Fluide ; widget (modes de puissance, Auto TDP, profil du jeu) ; préférence énergétique ; Game DVR (registre) ; pleine puissance (T1) ; étagère Processeur (T2) ; serveur de mouvement après changement de rendu ; visite rejouée qui garde `onboarded`.

## A. Finir et stabiliser

- [x] Page des optimisations Windows retravaillée.
- [x] « buttons: not supported » : plus d'erreur depuis le 29 octobre.
- [x] Étape optimisations de la visite.
- [x] Captures périmées refaites.
- [ ] **Lancer les jeux Steam sans la fenêtre de Steam.** Étudié (`docs/steam-launch-study.md`) : `-silent` ne suffit pas, la fenêtre apparaît encore avec Steam ouvert ou fermé.

## B. Fluidité de l'interface

- [x] Pleine puissance quand Canapé est devant (à éprouver : T1).
- [x] Rendu par la carte graphique en option (à éprouver : T12).
- [x] Animations allégées en éco (à éprouver : T1).

## C. Tirer le maximum de l'Ally, sans risque

- [x] Couper les services ASUS (à éprouver : T3).
- [x] Couper l'enregistrement en arrière-plan (Game DVR).
- [x] CPU boost, EPP, fréquence max, cœurs parqués par profil (à éprouver : T2).
- [x] Mouvement pour les émulateurs (serveur local) : validé avec un client local ; reprise du port après changement de rendu vérifiée.
- [ ] **Affinité des cœurs sur le Z1 non Extreme.** Fil principal du jeu sur les cœurs Zen 4.
- [ ] **Cache des shaders AMD au maximum.**
- [ ] **Comprendre l'option « GPU boost » d'autres outils.** À faire sur l'Ally avant de décider. Notes dans `docs/` seulement, rien dans le code.

## D. Image

- [ ] **Réglages AMD par profil (ADLX).** RSR et netteté, AFMF, Anti-Lag, Chill, RIS.
- [x] Définition d'écran par profil (à éprouver : T5).
- [ ] **Lossless Scaling.** Étude faite (`docs/lossless-scaling-study.md`). S'appuyer sur ses profils et son activation automatique, sans écrire dans son fichier de réglages.
- [x] Écran « Image » dans le profil d'un jeu (à éprouver : T5).
- [ ] **Interrupteur Lossless Scaling dans le widget.**

## E. Firmware et réglages avancés

- [ ] **Mémoire réservée à la puce graphique.** 4, 6 ou 8 Go par le firmware, avec redémarrage ; valeur vérifiée pour le modèle.
- [ ] **Undervolt et limite de température via PawnIO.** Test de stabilité guidé, retour à 0 après un plantage.
- [ ] **Fréquence minimale de la puce graphique.** Selon le point « GPU boost » de la section C.

## F. L'aboutissement

- [ ] **Banc d'essai qui combine tous les leviers.** Propose pour chaque jeu le meilleur rapport FPS par watt.

## G. Décisions, ménage et site

- [ ] **Protection de `main`.** La garder ou la retirer : toutes les sessions la contournent avec les droits de Pierre.
- [ ] **Adresse des commits.** Gmail ou koul.io.
- [ ] **Ancienne branche `claude/loving-cori-xyxqv6`.** Vérifier qu'elle est supprimée avant d'ouvrir le dépôt.
- [ ] **Site `canape-site` :** bancs sur d'autres jeux et sélecteur de jeux. Il faut la liste des jeux installés sur l'Ally.
- [ ] **Serveur de mouvement :** essai avec un vrai émulateur et accéléromètre. En attente, pas d'émulateur.
- [ ] **GOG (en pause).** Aucun compte connecté, aucun essai prévu. Retiré de l'interface le 29 octobre (réglages, visite, étagères, installation) : `GOG_ENABLED` dans `crates/canape-shell/src/library.rs` le remet quand on le reprend.

## Avant de pousser

`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, et `cargo check --target x86_64-pc-windows-msvc --all-targets` depuis Linux. Aucun secret dans le dépôt, aucune mention d'IA dans les commits.
