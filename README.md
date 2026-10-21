# Canapé

Interface « canapé » pour la ROG Ally, en Rust : elle démarre dans l'expérience plein écran (FSE) de Windows, pilote le matériel de l'Ally (TDP, ventilateurs, écran, batterie), ajuste le TDP automatiquement et propose un widget Xbox Game Bar pour les réglages rapides en jeu.

Objectif n° 1 : rester le plus léger possible.

| Dossier | Contenu |
|---|---|
| `crates/canape-core` | Protocole IPC et types partagés |
| `crates/canape-autotdp` | Algorithme d'auto TDP (testé sans matériel) |
| `crates/canape-hw` | Abstraction matérielle : backend Ally (Windows) et simulateur |
| `crates/canape-service` | Service privilégié : matériel, auto TDP, serveur IPC |
| `crates/canape-shell` | App d'accueil du FSE |
| `packaging/` | Paquet d'identité MSIX et script d'installation de dev |
| `widget/` | Widget Game Bar (à venir) |
| `docs/` | Architecture et analyse du démarrage FSE |

## Développer

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

Le service se teste en console, sur n'importe quel OS, avec le simulateur matériel :

```sh
echo '{"type":"set_tdp","watts":12}' | cargo run -p canape-service
```

Sur l'Ally, l'étape M1 (démarrer en FSE) est décrite dans [`packaging/README.md`](packaging/README.md). L'architecture complète est dans [`docs/architecture.md`](docs/architecture.md).
