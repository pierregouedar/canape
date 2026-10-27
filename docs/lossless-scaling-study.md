# Intégration de Lossless Scaling : étude

État au 27 octobre 2026 : étude sans accès à l'Ally ni à l'installation de Lossless Scaling. Les comportements de la version installée restent à vérifier.

## Ce que Canapé fait déjà

- `canape_library::steam::lossless_scaling` retrouve l'exécutable à partir du manifeste Steam et vérifie qu'il existe. Le shell met ce résultat en cache et propose les choix Lossless Scaling sur la page Image ; la détection est testée avec des fixtures, pas sur l'Ally.
- Les choix de mise à l'échelle et de génération d'images sont enregistrés dans le profil Canapé, mais ne sont pas appliqués. La résolution est gérée séparément par le shell.
- L'auto TDP utilise les FPS rapportés par AMD ADL pour l'écran, sans identifier le processus qui les produit. Avec une génération d'images active, on ne sait pas encore si cette mesure représente les images du jeu ou celles produites par Lossless Scaling : il ne faut pas en déduire automatiquement un objectif de FPS de base.

## Ce que les sources de l'éditeur établissent

L'[annonce de la version 2.0](https://store.steampowered.com/news/posts/?appgroupname=Lossless+Scaling&appids=993090&enddate=1717700585&feed=steam_community_announcements) décrit des profils de jeu et une option d'activation automatique. Elle indique aussi que le système de raccourcis de lancement de la [version 1.2.8](https://store.steampowered.com/news/posts/?appids=993090&enddate=1565616532&feed=steam_community_announcements) a été retiré en 2.0 : ces anciens arguments ne prouvent donc rien pour la version actuelle.

La [page Steam de l'application](https://store.steampowered.com/app/993090/Lossless_Scaling/) décrit la mise à l'échelle et la génération d'images ; les [annonces de l'éditeur](https://steamcommunity.com/app/993090/announcements/) décrivent notamment le coût GPU et les modes de génération. Elles ne documentent pas ici une API stable de lecture, d'écriture ou de synchronisation des profils avec Canapé.

## Proposition minimale

1. Sur l'Ally, noter la version installée et vérifier la détection existante. Configurer manuellement un profil dans Lossless Scaling et vérifier l'activation automatique pour un jeu, le retour à Canapé et la sortie du jeu.
2. Comparer les FPS de base affichés par le jeu ou Lossless Scaling avec les FPS ADL vus par Canapé, génération désactivée puis activée. Vérifier également l'effet du limiteur AMD avant de modifier l'auto TDP.
3. Si le profil et l'activation automatiques suffisent, conserver leur configuration dans Lossless Scaling et envisager seulement un lancement facultatif de l'application. Ne pas écraser son fichier de configuration sans format documenté et sans preuve qu'une modification reste compatible avec sa version.
4. Étudier ensuite la synchronisation des réglages et le widget si une interface fiable permet de connaître l'état et d'agir sans désynchroniser les deux applications. Tant que cela n'est pas établi, les choix de Canapé restent signalés comme non appliqués.

Les tests doivent utiliser la session interactive de l'Ally et être réalisés avec l'accord de Pierre avant lancement d'un jeu ou modification de l'installation.
