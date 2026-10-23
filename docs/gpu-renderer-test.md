# Essai : rendu logiciel ou rendu par la carte graphique

Le shell dessine avec le rendu logiciel de Slint (voir [architecture.md](architecture.md)). Cet essai mesure sur l'Ally ce que coûteraient les rendus par la carte graphique, avant de décider quoi que ce soit. La version normale du shell n'est pas concernée.

## Ce qu'on sait avant l'essai

- **Rendu logiciel** : 11 Mo privés au premier plan, 14 Mo avec un jeu devant (M4, M5). Sous Windows, il ne redessine que les zones qui changent.
- **OpenGL** : les 115 Mo de M2 mêlaient build debug et OpenGL ; jamais mesuré en release.
- **Direct3D** : dans Slint 1.18, seulement par wgpu (Direct3D 12), une option encore instable. Skia sous Windows passe par OpenGL ; il n'est pas dans l'essai (le plus lourd, et pas de Direct3D sans wgpu).
- **Fluidité** : le rendu logiciel n'était pas synchronisé avec l'écran. La copie à l'écran se fait par GDI, et le rythme vient d'un minuteur de 8 ms (125 Hz). Une saccade à 120 Hz pouvait donc venir de là, même quand le rendu est assez rapide. Le shell attend maintenant la composition suivante de Windows après chaque image (`vsync.rs`, `DwmFlush` après `WM_PAINT`) ; `CANAPE_NO_VSYNC=1` le désactive pour comparer.

## Version de test

Une seule version contient les trois rendus ; `CANAPE_RENDERER` choisit au lancement :

| `CANAPE_RENDERER` | Rendu | API |
|---|---|---|
| `software` (ou absent) | Logiciel | Processeur, copie GDI |
| `femtovg` | FemtoVG | OpenGL |
| `femtovg-wgpu` | FemtoVG | wgpu, Direct3D 12 |

```powershell
cargo build --release -p canape-shell --features gpu-renderers
```

Le rendu retenu est écrit dans `shell.log` (`renderer: …`).

## Mesures, pour chaque rendu

Toujours en release, sur la page Jeux, avec la vraie bibliothèque :

1. **Mémoire au repos**, 30 s après le lancement : mémoire privée (Private Bytes), ensemble de travail privé, et mémoire GPU dédiée et partagée (Gestionnaire des tâches, onglet Détails, colonnes à ajouter).
2. **Images par seconde** avec `SLINT_DEBUG_PERFORMANCE=refresh_full_speed,console` : navigation rapide pendant 15 s, d'abord gauche/droite seulement, puis haut/bas seulement.
3. **CPU** du shell : au repos sur Jeux pendant 20 s, puis pendant la navigation.
4. **Avec un jeu devant**, 1 min après son lancement : mémoire privée et mémoire GPU du shell, et FPS du jeu comparés à une partie sans le shell.
5. **Ressenti** : fluidité des animations à 120 Hz (Pierre), et netteté du texte et des jaquettes.
6. **Rendu logiciel seulement** : refaire 2, 3 et 5 avec `CANAPE_NO_VSYNC=1`, pour voir ce qu'apporte la synchronisation avec l'écran.

## Décision

On passe à un rendu par la carte graphique seulement s'il rend l'accueil fluide là où le rendu logiciel n'y arrive pas, sans dépasser l'objectif de 80 Mo et sans coûter de FPS en jeu. Sinon, on garde le rendu logiciel, synchronisé avec l'écran.

## Résultats

À remplir sur l'Ally.
