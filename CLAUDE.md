# Canapé : notes pour Claude

- Docs et échanges en français ; code, commentaires et messages de commit en anglais.
- Deux sessions travaillent sur ce dépôt : une sur la ROG Ally (Windows, peut compiler, lancer et tester le matériel et le FSE) et une dans le cloud (Linux, sans matériel). Synchroniser par GitHub ; ne jamais affirmer qu'un comportement Windows marche sans l'avoir testé sur l'Ally.
- Légèreté d'abord : pas de navigateur embarqué, pas de runtime async sans besoin mesuré, dépendances minimales. Voir les objectifs chiffrés dans docs/architecture.md.
- Seul `canape-service` a des droits élevés. `canape-shell` doit rester `asInvoker` : le FSE l'active en medium IL.
- Avant de pousser : `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, et `cargo check --target x86_64-pc-windows-msvc --all-targets` depuis Linux.
