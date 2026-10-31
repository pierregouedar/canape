# Lancement et installation Steam sans fenêtre

État au 29 octobre 2026 : lancement et mise en file d'une installation essayés sur l'Ally au commit `0f161d8`.

## Lancement d'un jeu installé

Depuis `2aac130`, `canape_library::steam::launch` prépare `steam.exe -silent steam://rungameid/<appid>` lorsque l'exécutable Steam existe. Sinon, Canapé transmet l'URI à Windows. Le test `launches_through_a_hidden_steam` vérifie ces deux commandes, pas l'apparence de Steam sur Windows.

Sur la RC71L, Among Us démarre depuis la fiche Canapé avec Steam déjà ouvert ou fermé. Avec Steam fermé, une fenêtre Steam « Démarrage du jeu » apparaît devant Canapé avant lui, malgré `-silent`. Avec Steam déjà ouvert, un relevé du processus au premier plan montre également `steamwebhelper` pendant environ 2,5 secondes avant le jeu, sans identifier le contenu de la fenêtre. Le détail est dans le [rapport de campagne](ally-alpha-report.md#lancement-steam-et-limite-de-fps). Le démarrage sans fenêtre reste donc à améliorer dans les deux cas.

Essai complémentaire avec Steam déjà ouvert : lancer directement la même commande avec `Start-Process -WindowStyle Hidden` donne encore `steamwebhelper` au premier plan pendant huit mesures à 250 ms avant le jeu. Masquer la fenêtre initiale du processus ne suffit donc pas à supprimer ce passage. Il faut préserver les éventuels dialogues utiles du lanceur, et vérifier toute autre approche sur l'Ally.

## Diagnostic et correctif du 31 octobre

Relevé sur l'Ally (Among Us, trois lancements depuis Canapé, `EnumWindows` toutes les ~110 ms, voir le [rapport](ally-alpha-report.md)) : une seule fenêtre Steam apparaît, « Lancement… », classe `SDL_app`, processus `steamwebhelper`, 600 × 286 au centre, et elle prend le premier plan à Canapé. Avec Steam ouvert, elle reste visible 2,8 s avant le jeu ; avec Steam fermé, environ 7 s de démarrage sans fenêtre, puis la même fenêtre 2,6 s. La fenêtre principale de Steam ne s'affiche jamais. La réduire (`SW_MINIMIZE`) dès qu'elle est vue n'empêche pas le jeu de démarrer.

Correctif (`steamveil.rs`) : après un lancement par Steam depuis Canapé, un fil surveille les fenêtres pendant 40 s et réduit toute fenêtre de classe `SDL_app`, de `steamwebhelper.exe` ou `steam.exe`, de moins de 800 × 400. Le client Steam, plus grand, n'est jamais touché. **À vérifier sur l'Ally** : que le jeu prenne bien le premier plan, et que le flash de moins de 100 ms avant la réduction ne se voie pas.

## Installation d'un jeu possédé

L'installation suit un autre chemin : Canapé essaie de mettre le téléchargement dans la file de Steam, puis démarre ou redémarre Steam discrètement lorsque c'est possible. Si la mise en file n'est pas possible ou ne démarre pas à temps, il utilise `steam://install/<appid>` et Steam peut afficher sa fenêtre d'installation. Le repli a été essayé sur l'Ally. Le 28 octobre, le journal montre aussi une installation de Toribash mise en file, prise en charge par Steam et terminée ; sans observation continue, elle ne prouve pas l'absence de fenêtre. Voir le [rapport de campagne](ally-alpha-report.md#lancement-steam-et-limite-de-fps) et le [guide utilisateur](user-guide.fr.md#installer-des-jeux-possédés).

## Vérification à faire sur l'Ally

Observer une installation de bout en bout pour vérifier les fenêtres et la progression affichée. Ne pas redémarrer Steam pendant un jeu. Pour le lancement, vérifier sur l'Ally toute modification destinée à supprimer la fenêtre, ainsi que l'arrivée du jeu au premier plan.
