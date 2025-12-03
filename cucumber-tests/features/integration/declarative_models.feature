# language: fr
Fonctionnalité: Modèles Déclaratifs Lithair
  En tant que développeur d'applications
  Je veux utiliser les modèles déclaratifs pour générer automatiquement les APIs CRUD
  Afin de réduire le code boilerplate et garantir la cohérence

  Contexte:
    Soit un serveur Lithair avec modèles déclaratifs activés
    Et que les permissions soient configurées automatiquement
    Et que les routes CRUD soient générées dynamiquement

  Scénario: Génération automatique des routes CRUD
    Quand je définis un modèle Article avec DeclarativeModel
    Alors les routes GET /articles, POST /articles, PUT /articles/{id}, DELETE /articles/{id} doivent être créées
    Et chaque route doit avoir les permissions appropriées
    Et le schéma JSON doit être généré automatiquement

  Scénario: Validation des permissions par modèle
    Quand un utilisateur "Contributor" accède à POST /articles
    Alors la requête doit être acceptée avec permission "ArticleWrite"
    Quand un utilisateur "Anonymous" accède à POST /articles
    Alors la requête doit être rejetée avec erreur 403 Forbidden
    Quand un utilisateur "Reporter" accède à GET /articles
    Alors la requête doit être acceptée avec permission "ArticleRead"

  Scénario: Persistance automatique des entités
    Quand je crée un article via POST /articles
    Alors l'article doit être persisté dans le state engine
    Et un ID unique doit être généré automatiquement
    Et les métadonnées de création doivent être ajoutées

  Scénario: Workflow d'états des entités
    Quand je crée un article avec statut "Draft"
    Et que je le mets à jour vers "Published"
    Alors le workflow doit respecter les transitions valides
    Et les hooks de cycle de vie doivent être exécutés
    Et l'état doit être validé avant sauvegarde

  Scénario: Relations entre modèles
    Quand je définis des modèles Article et Commentaire
    Et que Commentaire référence Article
    Alors les routes relationnelles doivent être générées
    Et /articles/{id}/comments doit être accessible
    Et la cohérence des références doit être garantie

  Scénario: Performance des requêtes déclaratives
    Quand j'effectue 1000 requêtes GET /articles en parallèle
    Alors toutes les requêtes doivent réussir
    Et le temps de réponse moyen doit être inférieur à 10ms
    Et la mémoire utilisée doit rester stable
