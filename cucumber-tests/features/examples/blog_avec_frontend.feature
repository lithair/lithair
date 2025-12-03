Feature: Blog Lithair avec Frontend

  # Configuration du serveur
  Background:
    Given un serveur Lithair avec les options:
      | option              | valeur                    |
      | port                | 0                         |
      | static_dir          | /tmp/blog-static          |
      | enable_sessions     | true                      |
      | session_store_path  | /tmp/blog-sessions        |
      | enable_admin        | true                      |

  Scenario: Le frontend HTML est accessible
    When je charge la page "/"
    Then je dois voir du HTML
    And le titre doit être "Mon Blog Lithair"
    And le CSS doit être chargé
    And le JavaScript doit être actif

  Scenario: Créer un article via l'API
    When je POST sur "/api/articles" avec:
      """json
      {
        "title": "Premier Article",
        "content": "Contenu de mon article",
        "author": "John Doe"
      }
      """
    Then la réponse doit être 201 Created
    And un ID unique doit être généré
    And l'article doit être persisté dans events.raftlog

  Scenario: Le frontend affiche les articles
    Given 3 articles créés via l'API
    When je charge la page "/"
    Then je dois voir 3 articles dans le DOM
    And chaque article doit avoir un titre
    And chaque article doit avoir un lien "Lire la suite"

  Scenario: Session utilisateur
    When je me connecte avec username "admin" et password "secret"
    Then je dois recevoir un cookie de session
    And le cookie doit être HttpOnly
    When je charge "/admin/dashboard"
    Then je dois voir le dashboard admin
    And je ne dois PAS voir "Login"

  Scenario: Frontend + Backend intégrés
    When je charge la page "/"
    And je clique sur "Créer un article" (JavaScript)
    And je remplis le formulaire avec:
      | champ   | valeur              |
      | titre   | Article via Frontend|
      | contenu | Contenu du frontend |
    And je soumets le formulaire
    Then une requête POST doit être envoyée à "/api/articles"
    And l'article doit apparaître dans la liste
    And l'article doit être en mémoire (StateEngine)
    And l'article doit être sur disque (FileStorage)
