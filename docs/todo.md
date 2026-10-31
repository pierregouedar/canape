# Canapé : feuille de route

État au 29 octobre 2026.

À lire avant de commencer : `AGENTS.md` à la racine du dépôt `pierregouedar/canape` (branche `main`). Ses règles s'appliquent à chaque point.

Méthode : on développe, puis on teste simplement sur l'Ally. Sur l'Ally, demander l'accord de Pierre avant tout lancement de jeu, installation, écriture sur la manette ou redémarrage. Ne jamais affirmer qu'un comportement Windows marche sans l'avoir observé.

## T. À éprouver sur l'Ally (du plus utile au moins utile)

Ces points ont été vérifiés comme absents des rapports d'essais (`docs/ally-alpha-report.md`, `docs/m5-report*.md`, `docs/gpu-renderer-test.md`).

- [x] **1. Pleine puissance dans Canapé et animations allégées.** Éprouvé le 29 octobre (rapport, T1). Reste : la fluidité ressentie, à juger par Pierre.
- [x] **2. Étagère Processeur depuis l'interface.** Éprouvé le 29 octobre (T2), libellés corrigés et revérifiés.
- [x] **3. Page Optimisations Windows refaite.** Éprouvé le 30 octobre sur `7c8b2fe` : Widgets « Indisponible », Rétablir, réapplication, liste Applications, état Windows remis (diff vide). Reste : libellé « Tout est appliq… » tronqué.
- [ ] **4. Limite de charge de la batterie.** Réglage par l'interface vérifié le 30 octobre, mais batterie à 100 % : l'arrêt et la reprise de la charge restent à observer avec la batterie sous 60 % (il faut débrancher).
- [x] **5. Définition d'écran par profil et écran Image.** Éprouvé le 30 octobre (720p au lancement, 1080p au retour, « Pas encore appliqué »). Reste : la fiche du jeu ne résume pas la définition.
- [x] **6. Options d'un jeu.** Renommer, catégories, retirer et réafficher éprouvés le 30 octobre ; clavier tactile corrigé et vérifié sur `284703f`.
- [x] **7. Ajouter d'autres jeux.** Éprouvé le 30 octobre. À revoir : A retire une entrée sans confirmation, et la liste remonte quand « Jeux masqués » disparaît.
- [ ] **8. Durées IGDB, succès Steam, images SteamGridDB, icônes.** Nécessitent les clés de Pierre.
- [ ] **9. Éclairage par l'interface et gyroscope avec palette.** Réglages éprouvés le 30 octobre ; reste l'essai physique (palette tenue ou basculée en bougeant la console dans un jeu) et la couleur réelle des sticks.
- [ ] **10. Widget.** Onglet Matériel, enregistrement et application au lancement éprouvés le 30 octobre. Reste : le bouton Armoury depuis un jeu (geste physique). À revoir : la limite de FPS n'agit qu'au lancement du jeu, et reste active après sa fermeture.
- [x] **11. Visite de bienvenue : toutes les étapes.** Éprouvé le 30 octobre sur `374c067` (9 étapes, rien appliqué, `onboarded` garde `true`).
- [x] **11 bis. Clavier tactile resté ouvert.** Corrigé et vérifié sur `7c8b2fe` (fermé après A/A et après B).
- [ ] **11 quater. Corrigé le 29 octobre, à revérifier sur l'Ally :** Widgets refusé par Windows (le tweak devient « verrouillé », la page ne reste plus bloquée), clavier tactile non détecté (fermeture forcée quand Canapé l'a ouvert), texte du dossier des jeux sans GOG. Reste : « 体中文 » et la police du chiffre « 1 à appliquer ».
- [x] **11 ter. Visite :** « 中文 » lisible, texte du dossier sans GOG (vérifié le 30 octobre).
- [x] **12. Rendu Fluide en jeu.** Mesuré le 30 octobre (environ 130 Mo de plus, CPU négligeable) ; repli sans programme GPU corrigé et vérifié sur `bf3c85d`. Reste : fluidité ressentie, annonce du repli à l'écran.
- [x] **13. Redémarrage de Windows.** Éprouvé le 30 octobre : service 15 s après le démarrage, shell lancé par l'écran Xbox 1 s après l'ouverture de session (journaux). Capture à l'écran non faite.
- [x] **14. Script de désinstallation** (`uninstall-dev.ps1`). Éprouvé le 30 octobre : tout est retiré, Windows rendu, données gardées ; réinstallation OK. À savoir : la réinstallation ne réapplique ni les optimisations ni l'étagère Processeur.

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

- [ ] **Réglages AMD par profil (ADLX).** Fait et essayé sur l'Ally le 30 octobre : RSR (avec sa netteté) et AFMF, appliqués par le shell pendant le jeu du profil et rendus à sa sortie (`radeon.rs`, registre identique avant et après). Anti-Lag, Chill et netteté de l'image (RIS) ajoutés le 30 octobre (carte Radeon, étagère AMD) ; essayés sur l'Ally le 31 : Chill et Anti-Lag marchent et le pilote est rendu à l'identique, Anti-Lag n'est pas gardé avec Chill, la netteté n'est pas appliquée avec Super Resolution (celle de Pierre est active : donc **netteté seule non essayée**). Reste : l'effet à l'œil.
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
