# Lancement et installation Steam sans fenêtre

État au 29 octobre 2026 : lancement et mise en file d'une installation essayés sur l'Ally au commit `0f161d8`.

## Lancement d'un jeu installé

Depuis `2aac130`, `canape_library::steam::launch` prépare `steam.exe -silent steam://rungameid/<appid>` lorsque l'exécutable Steam existe. Sinon, Canapé transmet l'URI à Windows. Le test `launches_through_a_hidden_steam` vérifie ces deux commandes, pas l'apparence de Steam sur Windows.

Sur la RC71L, Among Us démarre depuis la fiche Canapé avec Steam déjà ouvert ou fermé. Avec Steam fermé, une fenêtre Steam « Démarrage du jeu » apparaît devant Canapé avant lui, malgré `-silent`. Avec Steam déjà ouvert, un relevé du processus au premier plan montre également `steamwebhelper` pendant environ 2,5 secondes avant le jeu, sans identifier le contenu de la fenêtre. Le détail est dans le [rapport de campagne](ally-alpha-report.md#lancement-steam-et-limite-de-fps). Le démarrage sans fenêtre reste donc à améliorer dans les deux cas.

Essai complémentaire avec Steam déjà ouvert : lancer directement la même commande avec `Start-Process -WindowStyle Hidden` donne encore `steamwebhelper` au premier plan pendant huit mesures à 250 ms avant le jeu. Masquer la fenêtre initiale du processus ne suffit donc pas à supprimer ce passage. Il faut préserver les éventuels dialogues utiles du lanceur, et vérifier toute autre approche sur l'Ally.

## Installation d'un jeu possédé

L'installation suit un autre chemin : Canapé essaie de mettre le téléchargement dans la file de Steam, puis démarre ou redémarre Steam discrètement lorsque c'est possible. Si la mise en file n'est pas possible ou ne démarre pas à temps, il utilise `steam://install/<appid>` et Steam peut afficher sa fenêtre d'installation. Le repli a été essayé sur l'Ally. Le 28 octobre, le journal montre aussi une installation de Toribash mise en file, prise en charge par Steam et terminée ; sans observation continue, elle ne prouve pas l'absence de fenêtre. Voir le [rapport de campagne](ally-alpha-report.md#lancement-steam-et-limite-de-fps) et le [guide utilisateur](user-guide.fr.md#installer-des-jeux-possédés).

## Vérification à faire sur l'Ally

Observer une installation de bout en bout pour vérifier les fenêtres et la progression affichée. Ne pas redémarrer Steam pendant un jeu. Pour le lancement, vérifier sur l'Ally toute modification destinée à supprimer la fenêtre, ainsi que l'arrivée du jeu au premier plan.
