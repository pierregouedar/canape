# Packaging

`identity/` contient le paquet MSIX « sparse » qui donne une identité d'app à `canape-shell.exe`, pour que l'expérience plein écran (FSE) puisse l'utiliser comme app d'accueil. Le pourquoi est expliqué dans [`../docs/architecture.md`](../docs/architecture.md).

| Fichier | Rôle |
|---|---|
| `identity/AppxManifest.xml` | Identité `Canape`, extension `windows.gamingApp`, capacité `gamingHome` |
| `identity/CustomCapability.SCCD` | Débloque la capacité `gamingHome` (catalogue de test : mode développeur requis) |
| `identity/Assets/` | Logos provisoires |
| `../crates/canape-shell/canape-shell.exe.manifest` | Lie l'exe au paquet (`msix`) et le garde `asInvoker` ; intégré à la compilation |
| `install-dev.ps1` | Compile, installe le service `CanapeService`, signe et enregistre le paquet sur la machine de dev |

## M1 : démarrer Canapé en FSE sur l'Ally

Prérequis : mode développeur activé, Rust et Build Tools installés (le Windows SDK fournit `makeappx` et `signtool`).

1. Dans un **Terminal administrateur**, à la racine du dépôt :

   ```powershell
   powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1
   ```

2. Test hors FSE : lance la commande `explorer.exe shell:AppsFolder\...` affichée à la fin. L'interface de Canapé doit s'ouvrir en plein écran, et une ligne s'ajoute à `%LOCALAPPDATA%\Canape\shell.log`.
3. Choisis Canapé comme app d'accueil : Paramètres → Jeux → Expérience plein écran. S'il n'apparaît pas dans la liste, relance le script avec `-SetAsHomeApp`.
4. Active « démarrer dans l'expérience plein écran », redémarre l'Ally : Canapé doit s'afficher au démarrage.

Pour revenir à Winhanced ou à l'app Xbox, choisis-les à nouveau dans les mêmes Paramètres.

Le certificat de développement est créé dans `$HOME\dev\canape-cert` et ajouté à `LocalMachine\TrustedPeople`. Ne le commite jamais.

## Pièges sous Windows

- **Smart App Control** (activé sur l'Ally) peut bloquer les build scripts d'une compilation release (`os error 4551`). Chaque nouveau binaire non signé est jugé séparément : relancer passe parfois, pas toujours. Pour tester quand même, `install-dev.ps1 -BuildProfile debug` installe les binaires debug.
- **`canape-ctl` dans Windows PowerShell 5.1** : les guillemets doubles d'un argument passé à un exe sont perdus. Il faut les échapper :

  ```powershell
  & "C:\Program Files\Canape\canape-ctl.exe" '{\"type\":\"set_tdp\",\"watts\":12}'
  ```

## Builds release sans Smart App Control

La CI GitHub Actions (`.github/workflows/ci.yml`) compile les binaires release sous Windows et les publie comme artefact `canape-windows-x64`. Sur l'Ally :

```powershell
gh run download --repo pierregouedar/canape --name canape-windows-x64 --dir $HOME\dev\canape-bin
powershell -ExecutionPolicy Bypass -File packaging\install-dev.ps1 -BinariesDir $HOME\dev\canape-bin
```

## Vérifications automatisées (M5)

`m5-check.ps1 -Game <exe du jeu lancé>` vérifie la télémétrie (matériel détecté, consommation réelle, GPU, FPS), la couleur d'accent partagée, l'activation d'un profil et le TDP appris, un bench court, puis relève la mémoire du service, du shell et du widget. Il écrit les limites de puissance : à lancer avec l'accord du propriétaire, Winhanced en pause, un jeu (ou l'Aquarium WebGL en plein écran) au premier plan. Il remet ensuite l'état d'origine et écrit `results.md`, `bench.json` et `service-tail.log` dans `$HOME\dev\canape-m5`.
