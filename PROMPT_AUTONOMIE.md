# Prompt : rendre `new` autonome

Je veux transformer le répertoire `new` en projet complet, autonome et publiable sur GitHub. Le dossier parent contient le projet historique et tout le code nécessaire à la migration.

## Objectif

Réintégrer dans `new` le code métier et les ressources nécessaires pour que les téléchargements et les exports fonctionnent sans dépendre du dépôt parent, de ses exécutables compilés ou de chemins relatifs vers celui-ci. Le projet historique doit rester intact : toutes les modifications doivent rester dans `new`.

Le dépôt final doit pouvoir être cloné et compilé par une autre personne à partir des seuls fichiers de `new`. Les dépendances normales gérées par Cargo et les services de données accessibles sur Internet sont acceptables, mais doivent être identifiés et documentés. Ne pas présenter Blender, un CDN ou un service externe comme inclus si le code en dépend encore ; chercher à supprimer ces dépendances lorsque c’est raisonnable, sinon les signaler clairement.

## Travail demandé

1. Examiner le code, les configurations, les scripts, les ressources et les tests de `new` ainsi que du dossier parent. Repérer précisément les traitements historiques encore appelés, leurs données d’entrée, leurs exécutables, leurs bibliothèques et leurs ressources.
2. Établir une courte stratégie de migration fondée sur cet inventaire. Préserver les comportements et options existants ; ne pas réécrire un moteur géographique sans raison vérifiable.
3. Réintégrer dans `new` tout le code nécessaire aux opérations actuellement proposées : téléchargement, export STL, export STL par couches et export Blender. Privilégier des modules Rust réutilisables par l’API et les CLI plutôt que de lancer des exécutables externes issus du dossier parent.
4. Fournir des exécutables de ligne de commande distincts et clairement nommés pour les usages réellement présents dans le projet historique. Éviter les collisions de noms et documenter pour chaque binaire son rôle, ses arguments et un exemple d’appel. Conserver le serveur sous un binaire séparé.
5. Remplacer les chemins propres à une machine et les références au dossier parent par une configuration portable. Garder des valeurs locales sûres par défaut, ne pas intégrer de secrets et expliquer les dépendances qui restent nécessaires à l’exécution.
6. Mettre à jour le README pour un clone vierge : prérequis, configuration initiale, commandes de compilation et de lancement, exemples CLI, stockage, tests et limites connues. Ajouter ou modifier uniquement les fichiers nécessaires.

## Contraintes

- Ne modifier aucun fichier en dehors de `new`.
- Ne pas supprimer ni écraser les changements préexistants, même dans `new` ; les comprendre et travailler avec eux.
- Garder l’API actuelle compatible, sauf incompatibilité nécessaire à l’autonomie ; documenter alors la migration.
- Préserver les formats, les options d’export, le cache et le suivi des jobs lorsque cela est possible.
- Ne pas masquer les prérequis système restants. Si une fonctionnalité exige encore un outil externe, préciser laquelle, pourquoi, et quelles alternatives sont réalistes.
- Ne pas ajouter de licence ou de provenance supposée. Signaler les fichiers historiques dont la licence ou la réutilisation doit être vérifiée avant publication.
- Procéder par étapes : établir l’état initial, migrer un périmètre cohérent, lancer les vérifications ciblées, puis poursuivre. Ne pas conclure que le projet est autonome tant que les appels au dossier parent n’ont pas été éliminés et vérifiés.

## Validation attendue

- Depuis `new`, les commandes de compilation et de tests réussissent sur un clone propre, selon les prérequis documentés.
- Chaque binaire CLI compile séparément, affiche une aide exploitable et dispose d’un exemple dans le README.
- Les tests couvrent les comportements migrés importants ; les tests existants restent verts.
- Une recherche dans le dépôt confirme l’absence de dépendance à `..`, aux exécutables du projet historique et aux chemins absolus spécifiques à la machine, hors documentation explicitement historique.
- Le serveur et les CLI utilisent une configuration et un stockage cohérents.
- Le compte rendu final indique les fonctionnalités réintégrées, les commandes de validation exécutées, les dépendances encore nécessaires et les points bloquants éventuels pour une publication publique.