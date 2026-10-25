# Durée de jeu (HowLongToBeat) sur la fiche du jeu : étude

Objectif : afficher sur la fiche d'un jeu, dans la ligne des infos (« Joué hier · 12 h de jeu · 45 Go »), le temps moyen pour finir l'histoire principale, par exemple « Histoire ~25 h ». Contraintes de Canapé : un appel WinHTTP simple, un résultat mis en cache par jeu pendant des semaines, désactivé par défaut ou derrière un réglage, et un échec silencieux.

Résumé : **HowLongToBeat n'est pas implémenté.** Le site n'a pas d'API publique ; l'accès dont dépendent les bibliothèques open source change tous les deux ou trois mois et demande d'imiter un navigateur. C'est trop fragile pour un appel WinHTTP « posé une fois ». Si la fonction est voulue, la voie raisonnable est **IGDB** (données « time to beat » officielles, API documentée), avec une clé que l'utilisateur crée lui-même, comme pour SteamGridDB.

État : étude seule, rien dans le code. Recherche faite le 25 octobre 2026.

## HowLongToBeat aujourd'hui

- **Pas d'API officielle.** HowLongToBeat (howlongtobeat.com) ne publie ni API ni conditions d'accès pour des programmes ; tout accès automatisé passe par ce que le site web appelle lui-même.
- **Ce que font les bibliothèques open source** (par exemple `howlongtobeatpy`, la bibliothèque Python la plus suivie, version 1.0.24 de septembre 2026) :
  1. télécharger la page d'accueil et y repérer les scripts du site (`/_next/static/chunks/…`) ;
  2. télécharger ces scripts et y chercher, par expression régulière, l'appel `fetch("/api/…", { method: "POST" … })` qui donne le chemin de recherche du moment (il a déjà été `/api/search`, `/api/find`, `/api/s/`, `/api/finder/v2`…) ;
  3. demander un jeton à `/api/<chemin>/init` : la réponse donne un jeton et une paire clé / valeur dont **les noms eux-mêmes varient** ;
  4. envoyer la recherche en POST JSON (`searchType`, `searchTerms`, une vingtaine d'options) avec les en-têtes `x-auth-token`, `x-hp-key`, `x-hp-val`, un `Referer` et un `Origin` du site, et un **User-Agent de navigateur tiré au hasard** pour passer la protection anti-robots ;
  5. lire `comp_main` (secondes) dans le JSON de la réponse et choisir le bon jeu par similarité de titre.
- **Fréquence des ruptures** (tickets fermés de `howlongtobeatpy`) : août 2024, novembre 2024 (trois fois), décembre 2024, janvier 2025, avril 2025, novembre 2025 (« bot protection »), février 2026, mars 2026, mai 2026, août 2026 (passage des scripts du site à Turbopack, l'expression régulière ne trouvait plus l'appel). Soit **une panne tous les deux à trois mois**, chaque fois réparée par une nouvelle version de la bibliothèque. Une autre bibliothèque (C#) a dû ajouter en juillet 2026 le renouvellement du jeton au premier refus 403.

### Pourquoi ça ne convient pas à Canapé

| Critère | HowLongToBeat |
|---|---|
| Un appel WinHTTP simple | Non : 3 à 4 requêtes, dont l'analyse de scripts JavaScript de plusieurs centaines de Ko par expression régulière. |
| Robuste | Non : cassé tous les 2 à 3 mois ; chaque panne demanderait une nouvelle version de Canapé, alors que l'Ally n'a pas de mise à jour automatique. |
| Honnête vis-à-vis du site | Non : il faut se faire passer pour un navigateur (User-Agent aléatoire, en-têtes du site) et contourner une protection anti-robots que le site a mise en place exprès. Le site ne publie aucune autorisation pour cet usage. |
| Léger | Moyen : la mise en cache limite les appels, mais chaque recherche télécharge les scripts du site (ou il faut aussi mettre en cache le chemin et le jeton, qui expirent). |
| Échec silencieux | Possible, mais alors la fonction serait la plupart du temps simplement absente, sans que l'utilisateur sache pourquoi. |

Un échec silencieux tous les deux mois, sur une fonction que l'on ne peut pas réparer sans publier une nouvelle version, donne une fonction qui ne marche qu'une partie du temps. Le code qui imite un navigateur pour passer une protection anti-robots n'a pas sa place dans Canapé.

## Les autres sources

| Source | Données | Accès | Avis |
|---|---|---|---|
| **IGDB** (Twitch) | `game_time_to_beats` : `hastily`, `normally`, `completely` en secondes, et le nombre de contributions, par jeu IGDB. Les jeux IGDB se retrouvent depuis un identifiant Steam (`external_games`) ou par titre. | API documentée (api-docs.igdb.com), requêtes POST au format « Apicalypse ». Il faut une application Twitch (identifiant client + secret, compte Twitch avec double authentification), puis un jeton « client credentials » d'environ 60 jours obtenu par `POST https://id.twitch.tv/oauth2/token`. Limite : 4 requêtes par seconde. | **Recommandé** si la fonction est voulue. Stable, prévu pour ce genre d'usage. Les données viennent des joueurs d'IGDB : moins de jeux couverts et moins de contributions que HowLongToBeat. Conditions d'utilisation de l'API (usage non commercial, mention de la source) à relire avant de livrer. |
| **Steam** | Rien : ni la boutique ni l'API Web ne donnent de durée de jeu moyenne. | — | — |
| **Jeu de données figé** (export d'un site, base embarquée) | Durées pour les jeux connus à la date de l'export. | Embarquer ou télécharger un fichier. | Non : poids, droits sur les données, et vieillit vite. |
| **Service intermédiaire** (un serveur à nous qui interroge HowLongToBeat) | Celles de HowLongToBeat. | Un serveur à maintenir. | Non : déplace le problème sans le résoudre, et Canapé n'a pas de serveur. |

## Recommandation

1. **Ne pas implémenter HowLongToBeat.** Ni maintenant, ni sous une forme « expérimentale » : elle casserait sans prévenir.
2. **Si la durée de jeu est voulue, passer par IGDB**, sur le modèle de SteamGridDB :
   - réglage désactivé par défaut ; `igdb_client_id` et `igdb_client_secret` dans `shell.json`, créés par l'utilisateur sur dev.twitch.tv (le guide expliquerait comment) ; rien n'est envoyé ailleurs qu'à `id.twitch.tv` et `api.igdb.com` ;
   - jeton gardé dans le cache, redemandé à expiration ou au premier refus 401 ;
   - par jeu, une fois : jeu Steam → `external_games` par identifiant Steam, autres → recherche par titre (correspondance exacte avec `title_key`, sinon rien) ; puis `game_time_to_beats` ; résultat `normally` (ou « inconnu ») gardé dans `%LOCALAPPDATA%\Canape\igdb.json` avec sa date, redemandé au bout de 30 jours, au plus quelques dizaines de jeux par scan comme pour les jaquettes ;
   - fiche du jeu : « Histoire ~N h » ajouté à la ligne des infos seulement quand une valeur existe ; aucune erreur affichée, le détail dans `shell.log` ;
   - transport : le trait `Http` gagne un POST (WinHTTP le fait déjà, il suffit d'envoyer un corps) ; l'analyse des réponses reste pure et testée sur des réponses enregistrées, comme `online.rs`.
3. **Coût estimé** : un module de bibliothèque d'environ 250 lignes avec ses tests, le POST dans `http.rs`, une ligne de réglage, une chaîne traduite. Les noms exacts des champs IGDB sont à vérifier sur la documentation officielle au moment de l'écrire (elle n'était pas joignable depuis l'environnement de cette étude).

Décision attendue du propriétaire du projet : faut-il la durée de jeu au prix d'une deuxième clé à créer (Twitch / IGDB), ou laisser la fiche sans ?

## Sources

- `howlongtobeatpy` : code (`HTMLRequests.py` : découverte du chemin dans les scripts, `/init`, en-têtes `x-auth-token` / `x-hp-key` / `x-hp-val`, User-Agent aléatoire), notes de version 1.0.15 à 1.0.24 et tickets n° 25 à 60 sur GitHub (ScrappyCocco/HowLongToBeat-PythonAPI).
- Une bibliothèque C# (HowLongToBeatScraper, sur Codeberg) : point d'entrée `/api/search`, renouvellement du jeton au premier 403 (juillet 2026).
- IGDB : point d'accès `game_time_to_beats` (champs `hastily`, `normally`, `completely`), authentification par Twitch, limite de 4 requêtes par seconde, d'après des clients open source et des discussions de développeurs ; la documentation officielle (api-docs.igdb.com) est à relire avant toute implémentation.
