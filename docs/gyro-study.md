# Visée au gyroscope : étude

Objectif : viser en bougeant l'Ally (« gyro aiming »), comme le proposent d'autres outils et Steam sur d'autres consoles portables, sans casser les règles du projet : pas de pilote tiers, service léger, shell `asInvoker`.

Résumé : la seule voie qui tient sans pilote tiers est **gyro → souris** (`SendInput`), dans le **shell** (session de l'utilisateur), en lisant le capteur par l'**API capteurs de Windows** (`Windows.Devices.Sensors.Gyrometer`). L'émulation du stick droit demande un pilote de manette virtuelle (ViGEmBus, archivé) ou une commande du firmware de la manette qui n'est documentée nulle part. La voie souris est **codée** (voir [État](#état)) ; elle n'a **pas encore été essayée sur l'Ally**.

## État

| Étape | État |
|---|---|
| Étude | ✅ Ce document. |
| Gyro → souris | 🧪 Code écrit : lecture du gyroscope dans `canape-hw` (`motion.rs`, `gyro.rs` sous Windows, simulateur), réglages dans le protocole, `state.json` et les profils par jeu, injection par le shell pendant qu'un jeu est devant, sous-page **Gyroscope** depuis la page Matériel. Testé sous Linux avec le simulateur seulement, **pas encore essayé sur l'Ally**. |
| Serveur de mouvement (émulateurs) | 🧪 Code écrit : `motion_server.rs` dans le shell, réglage dans le tiroir du gyroscope, désactivé par défaut. Testé sous Linux (codage et aller-retour sur la boucle locale). **Essayé sur l'Ally le 29 octobre (`98b92e7`), sans émulateur** : le tiroir a cinq lignes, « Mouvement pour émulateurs » se bascule avec A, gauche et droite ; activé, le shell journalise `listening on 127.0.0.1:26760` et `netstat` ne montre le port que sur 127.0.0.1 ; désactivé, il journalise `stopped` et le port se ferme. Un client de test local (hors dépôt) reçoit la réponse de version (22 octets, `DSUS`, 1001) puis des paquets de 100 octets à environ 63 par seconde. Ally immobile, les trois vitesses restent vers 0,1 °/s ; quand Pierre tourne l'Ally, elles changent sur les trois axes et plafonnent à ±125 °/s pendant les mouvements rapides. L'ordre des axes n'a pas pu être établi : les mouvements étaient simultanés. L'accéléromètre reste à zéro. Le gyroscope n'est rouvert (`gyro: Windows gyrometer…` au journal) qu'après plus de 5 s sans demande, pas après 2 s : il est bien fermé quand plus personne ne demande. Non essayé avec un émulateur. |
| Gyro → stick droit | ⏳ Non retenu pour l'instant (voir [b](#b-émuler-le-stick-droit)). |

À vérifier en premier sur l'Ally : que `Gyrometer::GetDefault` trouve bien le capteur, la fréquence réelle des lectures, le **sens des axes** (gauche / droite, haut / bas), et que les palettes arrière envoient bien leurs codes HID quand la manette est en mode manette.

## Le capteur de l'Ally (RC71L)

| Point | Ce qu'on sait |
|---|---|
| Composant | IMU 6 axes **Bosch BMI323** (gyroscope + accéléromètre), dans le bloc de l'écran, relié au processeur et non au microcontrôleur de la manette. |
| Sous Windows | Pilote Bosch (« Bosch Accelerometer », version 1.0.1.7 constatée par des utilisateurs) qui expose le capteur à la **pile capteurs de Windows** : un *Gyrometer* et un *Accelerometer*, visibles dans le Gestionnaire de périphériques sous « Capteurs ». |
| Lecture | Deux API au-dessus de la même pile : l'ancienne API COM `ISensorManager` (Sensor API, `sensorsapi.h`), et l'API WinRT `Windows.Devices.Sensors.Gyrometer`, utilisable par une app de bureau Win32 sans identité de paquet ni capacité. Vitesses angulaires en **degrés par seconde** sur les trois axes. |
| Fréquence | Environ **100 Hz** sous Windows d'après les relevés d'utilisateurs (le même composant monte à 800 Hz sous Linux) . À mesurer sur l'Ally (`MinimumReportInterval` est écrit dans `shell.log`). |
| Repère | Celui de Windows pour l'orientation native de l'appareil (paysage pour l'Ally) : X vers la droite le long de l'écran, Y vers le haut de l'écran, Z sortant de l'écran vers le joueur. Les outils existants retournent X et Y et échangent Y et Z pour passer au repère d'une manette PlayStation à plat (lacet = rotation autour de Z), ce qui confirme que Windows rend les axes dans le repère de l'écran. |
| HID de la manette | Le microcontrôleur de la manette (VID `0x0B05`, PID `0x1ABE`) ne voit pas l'IMU : ses rapports d'entrée `0x5A` ne portent que des codes de boutons (Command Center, Armoury, palettes). Aucune donnée de mouvement à y lire. |
| Session 0 | Inutile : l'injection doit de toute façon se faire dans la session de l'utilisateur, donc la lecture aussi. |

## Les voies possibles

### a. Gyro → souris (`SendInput`)

| Point | Détail |
|---|---|
| Principe | On lit la vitesse angulaire, on l'intègre sur le temps écoulé, on la convertit en « comptes » de souris et on les envoie par `SendInput(MOUSEEVENTF_MOVE)` (mouvement relatif, sans accélération Windows). |
| Où | **Dans le shell** (`canape-shell.exe`, session 1, medium IL). `SendInput` n'injecte que dans le bureau de la session du processus appelant : impossible depuis le service (session 0, aucun bureau interactif). Un petit exe séparé n'apporterait rien : le shell tourne déjà en permanence dans la session et lit déjà les boutons de l'Ally. |
| À installer | Rien. Pas de pilote, pas de dépendance (vtables WinRT écrites à la main, comme les appels HID de `hid.rs`). |
| Licences | Rien de tiers. |
| Latence | Lecture à 100 Hz (10 ms) + intervalle du capteur (≈ 10 ms) : 10 à 20 ms de plus que la souris, comparable aux outils existants. |
| CPU | Une boucle de 10 ms, seulement **pendant qu'un jeu est devant et que le gyro est actif** (bouton tenu, ou mode « toujours »). Un appel WinRT, un `SendInput` et quelques multiplications par tour : bien en dessous de 0,1 % d'un cœur. Gyro coupé ou palettes relâchées : le fil dort sur une variable de condition, 0 %. |
| Jeux compatibles | Tous ceux qui acceptent la souris en même temps que la manette, ou la souris seule (FPS PC, jeux à la souris). Les jeux qui ne lisent **que** la manette l'ignorent ; certains basculent leurs icônes clavier / manette à chaque mouvement de souris. |
| Limites | UIPI : un processus medium IL ne peut pas injecter dans une fenêtre élevée (un jeu lancé en administrateur). Rien ne passe sur le bureau sécurisé (UAC, écran de verrouillage). |
| Anti-triche | Risque **faible** : un mouvement injecté porte le drapeau `LLMHF_INJECTED`, visible par un hook bas niveau, et arrive en Raw Input sans périphérique (`hDevice` nul). La plupart des anti-triche (EAC, BattlEye) ne bannissent pas pour ça ; quelques jeux compétitifs (Valorant / Vanguard) ignorent ou refusent l'entrée injectée. C'est le même mécanisme que celui des outils existants et des logiciels de souris. |

### b. Émuler le stick droit

| Voie | Ce qu'il faut | Licence | Installation | Verdict |
|---|---|---|---|---|
| **ViGEmBus** (manette Xbox 360 / DualShock 4 virtuelle) | Pilote noyau signé + client (ViGEmClient), plus **HidHide** pour cacher la vraie manette (sinon le jeu voit deux manettes). | BSD-3 (pilote), MIT (HidHide). | Deux pilotes noyau à installer en administrateur. | Le projet est **archivé depuis novembre 2023** (litige de marque) : il fonctionne encore, mais ne recevra plus que des correctifs ; son successeur annoncé (« VirtualPad ») n'est pas sorti. Contraire à la règle « pas de pilote tiers ». |
| **InputInjector** (`Windows.UI.Input.Preview.Injection`, `InitializeGamepadInjection`) | Capacité restreinte `inputInjectionBrokered` dans le paquet. | Windows. | Aucune, mais le paquet sparse doit déclarer la capacité. | La manette injectée n'apparaît qu'à `Windows.Gaming.Input`, pas à XInput ni DirectInput : la grande majorité des jeux PC ne la voient pas. À essayer un jour, sans grand espoir. |
| **vJoy** | Pilote DirectInput. | MIT. | Pilote. | Ancien, DirectInput seulement : ignoré par les jeux XInput. |
| Pilote maison | Un pilote de bus virtuel signé. | — | Signature attestation Microsoft. | Hors de portée d'un projet personnel. |

Toutes ces voies ajoutent une installation lourde ou ne touchent presque aucun jeu. Mais c'est la seule façon de viser au gyro dans les jeux qui ne lisent que la manette.

### c. Ce que propose le firmware de la manette

- Les commandes connues du rapport `0x5A` (`0xD1` puis : mode `0x01`, affectations `0x02`, zones mortes `0x04` / `0x05`, vibrations `0x06`, LED `0x08`, prêt `0x0A`, calibration `0x0D`, turbo `0x0F`, courbes de réponse `0x13`, anti-zone morte `0x18`) ne comportent **aucune commande gyro**. Aucune implémentation de référence publique (pilote Linux `hid-asus-ally`, travaux en cours dans `hid-asus`) n'en décrit.
- Le firmware de manette 316 (fin 2023) permet un gyro par jeu, « comme une souris » ou « comme un joystick ». Comme l'IMU n'est pas reliée au microcontrôleur, le logiciel constructeur lit le capteur sous Windows et envoie à la manette des commandes d'émulation par l'interface constructeur (voir la liste du noyau Linux, août 2024) ; le format de ces commandes n'est pas publié.
- Rien ne permettrait de vérifier ces commandes contre une implémentation de référence. **Écarté** tant qu'aucune source publique ne le documente. Si un jour une implémentation de référence décrit cette commande, ce serait la meilleure voie pour le stick droit : pas de pilote, le jeu voit la vraie manette.

## Recommandation

**Gyro → souris, dans le shell, actif pendant que les palettes arrière sont tenues.**

- C'est la seule voie sans pilote ni installation, et elle tient les objectifs de légèreté (rien au repos, une boucle de 10 ms pendant la visée).
- La lecture passe par `canape-hw` (trait `Gyro`, module `motion`) avec un simulateur pour les tests. Le trait `Device` reste celui du service : le gyroscope, comme l'écran et les boutons de l'Ally, appartient à la session de l'utilisateur.
- Les réglages appartiennent au service (protocole, `state.json`, profils par jeu), comme le reste ; le shell les applique, comme la fréquence de l'écran.
- L'émulation du stick droit reste ouverte : à reprendre si une commande firmware est documentée, ou si un pilote de manette virtuelle maintenu et léger apparaît.

## Activation et réglages

| Réglage | Valeurs | Par défaut | Pourquoi |
|---|---|---|---|
| Activation | **Coupé**, **Palettes maintenues**, **Palettes : bascule**, **Toujours** | Coupé | Tenir une palette est le geste le plus naturel (on vise en la tenant, on relâche pour recentrer). La bascule évite de la tenir longtemps. « Toujours » sert aux jeux joués entièrement au gyro. Les deux palettes ne se distinguent pas dans les rapports HID : l'une ou l'autre. Toucher le stick (sticks capacitifs) serait l'idéal, mais aucune donnée de contact n'est lisible. |
| Sensibilité | 1 à 50 comptes de souris par degré | 10 | Un quart de tour (90°) = 900 comptes à 10. La sensibilité du jeu fait le reste. |
| Axe horizontal | **Lacet** (tourner l'Ally autour de l'axe vertical) ou **Roulis** (le tourner comme un volant) | Lacet | Le lacet est le plus intuitif tenu devant soi ; le roulis évite de tourner les poignets quand on est allongé. |
| Inverser le vertical | Oui / Non | Non | Préférence de chacun. |

- **Seulement pendant un jeu** : le gyro n'agit que si le shell n'est pas au premier plan et qu'un jeu est suivi (`status.foreground`). Dans le shell, rien ne bouge.
- **Par jeu** : le profil d'un jeu peut porter ses propres réglages (`Profile::gyro`), sinon il suit les réglages globaux, comme la limite de FPS. **X** sur la sous-page les enregistre pour le jeu au premier plan.
- **Bruit et dérive** : une petite zone morte douce (0,75 °/s retranchés de la vitesse) absorbe le bruit au repos sans couper les mouvements lents ; les fractions de comptes sont gardées d'une lecture à l'autre, pour que les petits mouvements ne se perdent pas. Pas de calibration de dérive pour l'instant : avec l'activation par palette, la dérive ne joue que pendant la visée. À revoir après l'essai.

## Sources

- [Forum Bosch Sensortec : « BMI323 Windows Driver only sampling 100hz? »](https://community.bosch-sensortec.com/mems-sensors-forum-jrmujtaw/post/bmi323-windows-driver-only-sampling-100hz-5HB2oMzlVvc2ZZS)
- [hid-asus-ally](https://github.com/uejji/hid-asus-ally) (pilote Linux) : liste des commandes `0xD1` de la manette.
- [Lowyat](https://lowyat.net/2023/312064/latest-asus-rog-ally-now-features-gyroscope-control), [Notebookcheck](https://www.notebookcheck.net/ASUS-ROG-Ally-gains-gyroscope-controls-and-other-new-features-with-new-updates.776656.0.html) : le gyro avec le firmware de manette 316.
- [Liste linux-input, « hid-asus-ally: Add full gamepad support », réponse d'Antheas Kapenekakis (août 2024)](https://lkml.iu.edu/hypermail/linux/kernel/2408.1/00923.html) : le gyroscope est émulé en envoyant des commandes à la manette.
- [ViGEmBus](https://github.com/nefarius/ViGEmBus) : archivé le 2 novembre 2023.
- Microsoft : [`Windows.Devices.Sensors.Gyrometer`](https://learn.microsoft.com/uwp/api/windows.devices.sensors.gyrometer), [`SendInput`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendinput), [repère des capteurs](https://learn.microsoft.com/windows-hardware/design/whitepapers/integrating-motion-and-orientation-sensors).
