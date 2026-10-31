# Canapé : feuille de route

État au 31 octobre 2026.

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
- [x] **7. Ajouter d'autres jeux.** Éprouvé le 30 octobre. Corrigé le 31 octobre (`8e05ec5`) : A demande une seconde pression avant de retirer, le curseur reste sur son jeu ; à revérifier sur l'Ally.
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
- [ ] **Lancer les jeux Steam sans la fenêtre de Steam.** Étudié (`docs/steam-launch-study.md`) : `-silent` ne suffit pas ; fenêtre identifiée le 31 octobre et réduite par `steamveil.rs` (à essayer sur l'Ally).

## B. Fluidité de l'interface

- [x] Pleine puissance quand Canapé est devant (à éprouver : T1).
- [x] Rendu par la carte graphique en option (à éprouver : T12).
- [x] Animations allégées en éco (à éprouver : T1).

## C. Tirer le maximum de l'Ally, sans risque

- [x] Couper les services ASUS (à éprouver : T3).
- [x] Couper l'enregistrement en arrière-plan (Game DVR).
- [x] CPU boost, EPP, fréquence max, cœurs parqués par profil (à éprouver : T2).
- [x] Mouvement pour les émulateurs (serveur local) : validé avec un client local ; reprise du port après changement de rendu vérifiée.
- [x] **Affinité des cœurs sur le Z1 non Extreme.** Sans objet sur cet Ally : c'est un Z1 Extreme (8 cœurs Zen 4, 16 logiques). À reprendre seulement avec un Z1 simple sous la main.
- [x] **Cache des shaders AMD au maximum.** Relevé le 31 octobre : `UMD\ShaderCache` vaut déjà « 1 » (réglage par défaut du pilote) et ADLX n'expose que la remise à zéro (`IADLX3DResetShaderCache`) : rien à régler de plus. Abandonné.
- [ ] **Comprendre l'option « GPU boost » d'autres outils.** Relevé le 31 octobre : aucune valeur MinClock/MinGfx/Tuning dans le pilote ; ADLX contient `IADLXManualGraphicsTuning` (réglage manuel, de type overclocking) et `IADLX3DBoost` (Radeon Boost, `KMD_RadeonBoostEnabled`=0), non appelés. Rien dans le code ; décision à prendre avec Pierre avant toute écriture de fréquence.

## D. Image

- [ ] **Réglages AMD par profil (ADLX).** Fait et essayé sur l'Ally le 30 octobre : RSR (avec sa netteté) et AFMF, appliqués par le shell pendant le jeu du profil et rendus à sa sortie (`radeon.rs`, registre identique avant et après). Anti-Lag, Chill et netteté de l'image (RIS) ajoutés le 30 octobre (carte Radeon, étagère AMD) ; essayés sur l'Ally le 31 : Chill et Anti-Lag marchent et le pilote est rendu à l'identique, Anti-Lag n'est pas gardé avec Chill, la netteté n'est pas appliquée avec Super Resolution (celle de Pierre est active : donc **netteté seule non essayée**). Reste : l'effet à l'œil.
- [x] Définition d'écran par profil (à éprouver : T5).
- [ ] **Lossless Scaling.** Relevé fait le 31 octobre (profils dans `Settings.xml`, pas de ligne de commande, voir l'étude) : Canapé n'écrit pas dans ce fichier. Reste l'essai des FPS avec et sans génération d'images (accord de Pierre). Étude faite (`docs/lossless-scaling-study.md`). S'appuyer sur ses profils et son activation automatique, sans écrire dans son fichier de réglages.
- [x] Écran « Image » dans le profil d'un jeu (à éprouver : T5).
- [ ] **Interrupteur Lossless Scaling dans le widget.**

## E. Firmware et réglages avancés

- [ ] **Mémoire réservée à la puce graphique.** 4, 6 ou 8 Go par le firmware, avec redémarrage ; valeur vérifiée pour le modèle.
- [ ] **Undervolt et limite de température via PawnIO.** Test de stabilité guidé, retour à 0 après un plantage.
- [ ] **Fréquence minimale de la puce graphique.** Selon le point « GPU boost » de la section C.

## F. L'aboutissement

- [ ] **Banc d'essai qui combine tous les leviers.** Première étape faite le 31 octobre : option « Comparer le boost du processeur » (seconde passe avec le boost coupé, ligne « Mieux sans le boost » dans les résultats ; boost rendu à la fin ou à l'annulation, testé en simulation). Pas encore essayé sur l'Ally. Reste : limite de FPS, fréquence d'écran et cœurs parqués comme autres leviers.

## G. Décisions, ménage et site

- [ ] **Protection de `main`.** La garder ou la retirer : toutes les sessions la contournent avec les droits de Pierre.
- [ ] **Adresse des commits.** Gmail ou koul.io.
- [ ] **Ancienne branche `claude/loving-cori-xyxqv6`.** Vérifier qu'elle est supprimée avant d'ouvrir le dépôt.
- [ ] **Site `canape-site` :** bancs sur d'autres jeux et sélecteur de jeux. Jeux installés relevés le 31 octobre (16 : 12 Steam, 2 Epic, Palworld, Heaven Benchmark) ; il manque des bancs mesurés, ce qui demande de lancer des jeux avec l'accord de Pierre. Sans mesures, un sélecteur serait vide : on ne l'ajoute pas.
- [ ] **Serveur de mouvement :** essai avec un vrai émulateur. Accéléromètre : lu (1,01 g à plat le 31 octobre). Le plantage du premier essai (violation d'accès 6–7 s après l'abonnement) venait de `get_object` (`then_some` construisait puis relâchait un objet nul quand le capteur n'avait pas encore de lecture) : corrigé le 31 octobre et vérifié sur l'Ally (63 paquets par seconde, aucune accélération nulle, lecture sur un fil à part). À voir avec quelqu'un qui bouge la console : la vitesse angulaire au repos reste strictement constante pendant une session (valeur retenue par Windows ?) et le sens des axes. En attente, pas d'émulateur.
- [ ] **GOG (en pause).** Aucun compte connecté, aucun essai prévu. Retiré de l'interface le 29 octobre (réglages, visite, étagères, installation) : `GOG_ENABLED` dans `crates/canape-shell/src/library.rs` le remet quand on le reprend.

## H. Petits correctifs d'ergonomie (31 octobre)

- [x] « Ajouter un jeu » : confirmation avant de retirer, curseur stable (`8e05ec5`).
- [x] Annoncer à l'écran le repli du rendu GPU vers le rendu logiciel (bandeau de 10 s au démarrage ; vérifié sur l'Ally le 31 octobre sur `1618e1b`, libellé aligné sur « Économe » ensuite).
- [x] Options d'un jeu : « Retirer » d'un jeu de boutique ne fait que le masquer (réversible, dit à l'écran) : pas de confirmation ajoutée, seul le retrait de `games.json` en demande une.
- [ ] Libellé « Tout est appliqué » tronqué sur la page des optimisations.
- [x] Fiche du jeu : la définition d'écran et l'image du profil sont déjà résumées (`profile.screen`, `profile.image`).
- [x] Réinstallation : dit clairement dans le guide FR que les optimisations et l'étagère Processeur ne se réappliquent pas seules (rendues à la désinstallation, par conception).

## I. Demandes de Pierre du 31 octobre (en cours)

- [ ] **Lancement Steam sans fenêtre.** Vérifié sur l'Ally le 31 octobre sur `950f5d3` : fenêtre réduite dès le premier relevé quand Steam est ouvert (résolu) ; à froid, la boîte reste ~1,3 s au premier plan : relevé toutes les 20 ms et retour du premier plan à Canapé ajoutés, à revérifier. Reste : le bouton affiche « Jouer » et non « Lancement… » pendant l'attente de Steam. Premier essai le 31 octobre (`fc531aa`) : fenêtre non réduite, limite de taille trop basse (900 × 429 px réels à 150 %) ; limite portée à 1100 × 560, à réessayer. Diagnostic fait sur l'Ally (fenêtre « Lancement… », `SDL_app`, 600 × 286, réductible sans gêner le jeu) ; correctif codé le 31 octobre (`steamveil.rs`, réduit la fenêtre pendant 40 s après un lancement). À essayer sur l'Ally : premier plan du jeu et flash éventuel.
- [x] **Matériel mieux organisé.** Fait le 31 octobre : quatre étagères (Refroidissement et batterie, Manette, Affichage, Image) ; le pilote AMD rejoint l'étagère Image, la batterie le refroidissement. Vérifié par captures sous Linux ; à voir sur l'Ally.
- [ ] **Courbes de ventilateurs personnalisées.** Éditeur codé le 31 octobre (page « Courbes de ventilateurs », quatrième tuile du tiroir Ventilateurs, `fancurve.rs`, vérifié par captures sous Linux). Reste : l'essayer sur l'Ally, et l'enregistrement par profil (X de la page Matériel garde déjà les courbes courantes).
- [ ] **Ajout de jeux dans la vue « Jeux ».** Première étape faite le 31 octobre : X dans Jeux ouvre « Gérer les jeux » (ordre des étagères Titre / Dernier lancement, jeux masqués, ajout et retrait des autres jeux). Reste : à essayer sur l'Ally ; autres tris (par boutique, temps de jeu) si Pierre en veut.
- [ ] **Succès Steam pour les jeux hors Steam.** Codé le 31 octobre : recherche dans le magasin Steam d'un titre identique (`match_appid`), liste de ses succès verrouillés, résultat gardé dans `steam-matches.json`. À essayer sur l'Ally avec Fall Guys ou art of rally ; les succès réellement débloqués d'une copie Epic ne sont pas lisibles.
- [ ] **Installation en un clic d'émulateurs.** Étude écrite le 31 octobre (`docs/emulator-install-study.md`) : sources officielles, vérification, dossier à part, intégration à la bibliothèque. À décider avec Pierre : quels émulateurs d'abord, zip seulement ou aussi 7z. Adresses et noms de fichiers à vérifier avant de coder.

## Avant de pousser

`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, et `cargo check --target x86_64-pc-windows-msvc --all-targets` depuis Linux. Aucun secret dans le dépôt, aucune mention d'IA dans les commits.
