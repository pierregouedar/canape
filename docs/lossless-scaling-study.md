# Intégration de Lossless Scaling : étude

État au 27 octobre 2026 : étude sans accès à l'Ally ni à l'installation de Lossless Scaling. Les comportements de la version installée restent à vérifier.

## Ce que Canapé fait déjà

- `canape_library::steam::lossless_scaling` retrouve l'exécutable à partir du manifeste Steam et vérifie qu'il existe. Le shell met ce résultat en cache et propose les choix Lossless Scaling sur la page Image ; la détection est testée avec des fixtures, pas sur l'Ally.
- Les choix de mise à l'échelle et de génération d'images sont enregistrés dans le profil Canapé, mais ne sont pas appliqués. La résolution est gérée séparément par le shell.
- L'auto TDP utilise les FPS rapportés par AMD ADL pour l'écran, sans identifier le processus qui les produit. Avec une génération d'images active, on ne sait pas encore si cette mesure représente les images du jeu ou celles produites par Lossless Scaling : il ne faut pas en déduire automatiquement un objectif de FPS de base.

## Ce que les sources de l'éditeur établissent

L'[annonce de la version 2.0](https://store.steampowered.com/news/posts/?appgroupname=Lossless+Scaling&appids=993090&enddate=1717700585&feed=steam_community_announcements) décrit des profils de jeu et une option d'activation automatique. Elle indique aussi que le système de raccourcis de lancement de la [version 1.2.8](https://store.steampowered.com/news/posts/?appids=993090&enddate=1565616532&feed=steam_community_announcements) a été retiré en 2.0 : ces anciens arguments ne prouvent donc rien pour la version actuelle.

La [page Steam de l'application](https://store.steampowered.com/app/993090/Lossless_Scaling/) décrit la mise à l'échelle et la génération d'images ; les [annonces de l'éditeur](https://steamcommunity.com/app/993090/announcements/) décrivent notamment le coût GPU et les modes de génération. Elles ne documentent pas ici une API stable de lecture, d'écriture ou de synchronisation des profils avec Canapé.

## Relevé sur l'Ally (31 octobre 2026)

Lecture seule, aucun jeu lancé.

- Installé par Steam (appid 993090), version 3.2.2.0 ; Canapé le détecte par le manifeste Steam.
- Réglages : `%LOCALAPPDATA%\Lossless Scaling\Settings.xml` (XML). Raccourci `Alt+Ctrl+S`, démarrage avec Windows désactivé. `GameProfiles` contient un seul profil « Par défaut » (`AutoScale=false`, `ScalingType=Off`, `FrameGeneration=Off`, `LSFG3Multiplier=2`, `LSFG3Target=120`, `ScaleFactor=1.5`, `CaptureApi=DXGI`). Les champs `Executable`/`ExecutableFiles`, `ProcessName` et `WindowTitle` existent dans le code et lient un profil à un jeu. `config.ini` à côté de l'exécutable ne contient pas de profils. Rien dans `%APPDATA%` ni dans le registre utilisateur.
- Aucun argument de ligne de commande trouvé. Seuls moyens visibles : le raccourci clavier et l'échelle automatique par profil.

**FPS avec AFMF (Among Us, écran fixe, 30 relevés) :** le compteur ADL donne 60,0 FPS avec et sans AFMF (8,1 W sans, 12,2 W avec) : il compte ce que le jeu présente, pas les images générées ; les +4,1 W montrent que le pilote travaille. Limites : image fixe, un seul jeu. Pour mesurer les images affichées, il faudrait PresentMon ou la superposition AMD. La génération d'images de Lossless Scaling n'est pas essayée. Conséquence : l'auto TDP ne doit pas déduire un objectif de FPS de base de ce compteur quand une génération d'images est active.

**Décision :** Canapé n'écrit pas dans `Settings.xml` (fichier d'une autre application, réécrit à sa fermeture, format non documenté). Il lance Lossless Scaling si besoin et laisse l'utilisateur créer le profil du jeu avec l'échelle automatique. L'interrupteur du widget (D20) attend l'essai des FPS avec et sans génération d'images, qui demande l'accord de Pierre pour lancer un jeu.

## Proposition minimale

1. Sur l'Ally, noter la version installée et vérifier la détection existante. Configurer manuellement un profil dans Lossless Scaling et vérifier l'activation automatique pour un jeu, le retour à Canapé et la sortie du jeu.
2. Comparer les FPS de base affichés par le jeu ou Lossless Scaling avec les FPS ADL vus par Canapé, génération désactivée puis activée. Vérifier également l'effet du limiteur AMD avant de modifier l'auto TDP.
3. Si le profil et l'activation automatiques suffisent, conserver leur configuration dans Lossless Scaling et envisager seulement un lancement facultatif de l'application. Ne pas écraser son fichier de configuration sans format documenté et sans preuve qu'une modification reste compatible avec sa version.
4. Étudier ensuite la synchronisation des réglages et le widget si une interface fiable permet de connaître l'état et d'agir sans désynchroniser les deux applications. Tant que cela n'est pas établi, les choix de Canapé restent signalés comme non appliqués.

Les tests doivent utiliser la session interactive de l'Ally et être réalisés avec l'accord de Pierre avant lancement d'un jeu ou modification de l'installation.
