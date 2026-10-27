# Lancement et installation Steam sans fenêtre

État au 27 octobre 2026 : analyse du code et des rapports, sans essai sur l'Ally.

## Lancement d'un jeu installé

Depuis `2aac130`, `canape_library::steam::launch` prépare `steam.exe -silent steam://rungameid/<appid>` lorsque l'exécutable Steam existe. Sinon, Canapé transmet l'URI à Windows. Le test `launches_through_a_hidden_steam` vérifie ces deux commandes, pas l'apparence de Steam sur Windows.

Le [rapport M5](m5-report-2.md#steam-install) constate un lancement d'Among Us depuis une version antérieure utilisant l'URI seule. Il ne valide donc pas le comportement du lancement `-silent` actuel. Sans savoir si la fenêtre gênante apparaît au démarrage de Steam, lorsqu'il tourne déjà, au lancement du jeu ou lors d'une installation, changer la commande serait spéculatif.

## Installation d'un jeu possédé

L'installation suit un autre chemin : Canapé essaie de mettre le téléchargement dans la file de Steam, puis démarre ou redémarre Steam discrètement lorsque c'est possible. Si la mise en file n'est pas possible ou ne démarre pas à temps, il utilise `steam://install/<appid>` et Steam peut afficher sa fenêtre d'installation. Le repli a été essayé sur l'Ally ; le chemin sans fenêtre reste à essayer. Voir le [guide utilisateur](user-guide.fr.md#installer-des-jeux-possédés).

## Vérification à faire sur l'Ally

Avec l'accord de Pierre et en notant le commit : essayer le lancement d'un jeu déjà installé avec Steam fermé puis déjà ouvert ; noter quelle fenêtre apparaît et quand. Tester séparément l'installation d'un jeu possédé, observer si le repli est utilisé et consulter `shell.log`. Ne pas redémarrer Steam pendant un jeu. Ces résultats permettront de décider si le point 5 demande encore du code.
