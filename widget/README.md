# Widget Xbox Game Bar (M3)

Les widgets Game Bar sont des apps UWP qui tournent en AppContainer : ils ne peuvent pas être écrits raisonnablement en Rust. Ce dossier accueillera une coquille C# minimale :

- affichage : TDP, mode, FPS, auto TDP, luminosité ;
- aucune logique matérielle : chaque action devient une requête JSON (`canape-core::protocol`) envoyée au service sur `\\.\pipe\canape` ;
- la DACL du pipe autorise le SID du paquet du widget (voir `docs/architecture.md`).
