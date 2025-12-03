@doc
Feature: Toutes les Configurations Lithair Possibles

  # Ce fichier contient TOUS les scénarios de configuration possibles
  # pour valider que Lithair fonctionne dans TOUS les cas d'usage

  # ==================== CONFIGURATION 1 : API SEULE + PERSISTENCE ====================

  Scenario: API seule avec persistence sur disque
    Given un serveur Lithair avec configuration:
      | option              | valeur                  |
      | mode                | api_only                |
      | persistence         | enabled                 |
      | persistence_path    | /tmp/lithair-api      |
      | port                | 0                       |
      | frontend            | disabled                |
    When je crée 10 articles via l'API
    Then tous les articles doivent être en mémoire
    And tous les articles doivent être sur disque dans events.raftlog
    And je peux redémarrer le serveur
    And les 10 articles doivent être rechargés depuis le disque

  # ==================== CONFIGURATION 2 : API SEULE SANS PERSISTENCE ====================

  Scenario: API seule en mode in-memory (pas de persistence)
    Given un serveur Lithair avec configuration:
      | option              | valeur                  |
      | mode                | api_only                |
      | persistence         | disabled                |
      | port                | 0                       |
      | frontend            | disabled                |
    When je crée 100 articles via l'API
    Then tous les articles doivent être en mémoire uniquement
    And aucun fichier ne doit être créé sur disque
    And les performances doivent être maximales
    When je redémarre le serveur
    Then toutes les données doivent être perdues

  # ==================== CONFIGURATION 3 : FRONTEND + API + PERSISTENCE ====================

  Scenario: Application complète avec frontend HTML/JS
    Given un serveur Lithair avec configuration:
      | option              | valeur                    |
      | mode                | full_stack                |
      | persistence         | enabled                   |
      | persistence_path    | /tmp/lithair-full       |
      | port                | 3000                      |
      | frontend            | enabled                   |
      | static_dir          | /tmp/lithair-static     |
      | enable_sessions     | true                      |
    When je charge la page "/"
    Then je dois voir le frontend HTML
    And le CSS doit être chargé
    And le JavaScript doit fonctionner
    When je crée un article depuis le frontend
    Then l'article doit être visible dans le DOM
    And l'article doit être dans l'API
    And l'article doit être persisté sur disque

  # ==================== CONFIGURATION 4 : MODE PRODUCTION ====================

  Scenario: Mode production avec toutes les optimisations
    Given un serveur Lithair avec configuration:
      | option              | valeur                    |
      | mode                | production                |
      | persistence         | enabled                   |
      | persistence_path    | /var/lib/lithair        |
      | port                | 80                        |
      | tls                 | enabled                   |
      | tls_cert            | /path/to/cert.pem         |
      | tls_key             | /path/to/key.pem          |
      | rate_limiting       | 1000                      |
      | cache               | enabled                   |
      | cache_ttl           | 3600                      |
      | max_connections     | 10000                     |
      | enable_metrics      | true                      |
      | metrics_port        | 9090                      |
    When je fais 10000 requêtes concurrentes
    Then toutes doivent réussir
    And la latence moyenne doit être < 50ms
    And le rate limiting doit bloquer après 1000 req/min
    And les métriques Prometheus doivent être disponibles

  # ==================== CONFIGURATION 5 : MODE DÉVELOPPEMENT ====================

  Scenario: Mode développement avec hot reload et debug
    Given un serveur Lithair avec configuration:
      | option              | valeur                    |
      | mode                | development               |
      | persistence         | enabled                   |
      | port                | 3000                      |
      | debug               | true                      |
      | hot_reload          | true                      |
      | cors                | *                         |
    When je modifie un fichier source
    Then le serveur doit se recharger automatiquement
    And les logs de debug doivent être visibles
    And CORS doit autoriser toutes les origines

  # ==================== CONFIGURATION 6 : CLUSTER DISTRIBUÉ ====================

  Scenario: Cluster Lithair 5 nœuds avec réplication
    Given un cluster Lithair avec configuration:
      | option              | valeur                    |
      | mode                | cluster                   |
      | node_count          | 5                         |
      | persistence         | enabled                   |
      | replication_factor  | 3                         |
      | consensus           | raft                      |
    When j'écris 1000 articles sur le nœud leader
    Then les données doivent être répliquées sur 3 nœuds minimum
    When le leader tombe en panne
    Then un nouveau leader doit être élu en < 5 secondes
    And le cluster doit continuer à fonctionner
    And aucune donnée ne doit être perdue

  # ==================== CONFIGURATION 7 : ADMIN PANEL ====================

  Scenario: Panneau d'administration avec authentification
    Given un serveur Lithair avec configuration:
      | option              | valeur                    |
      | mode                | full_stack                |
      | admin_panel         | enabled                   |
      | admin_port          | 8080                      |
      | auth                | jwt                       |
      | admin_user          | admin                     |
      | admin_password      | secret123                 |
    When je me connecte au panel admin avec "admin" / "secret123"
    Then je dois accéder au dashboard
    And je dois voir les métriques en temps réel
    And je dois pouvoir gérer les utilisateurs
    And je dois pouvoir voir les logs
    And je dois pouvoir faire un backup

  # ==================== CONFIGURATION 8 : API + SSO ====================

  Scenario: API avec Single Sign-On (Google OAuth)
    Given un serveur Lithair avec configuration:
      | option              | valeur                    |
      | mode                | api_only                  |
      | auth                | oauth2                    |
      | oauth_provider      | google                    |
      | oauth_client_id     | xxx.apps.googleusercontent.com |
      | oauth_client_secret | secret                    |
      | oauth_redirect_uri  | http://localhost:3000/callback |
    When un utilisateur se connecte avec Google
    Then il doit recevoir un token JWT
    And le token doit être valide
    And l'utilisateur doit accéder à l'API protégée

  # ==================== CONFIGURATION 9 : MICROSERVICES ====================

  Scenario: Lithair en mode microservice
    Given plusieurs serveurs Lithair:
      | service         | port | config                    |
      | users-service   | 3001 | api_only, persistence     |
      | articles-service| 3002 | api_only, persistence     |
      | auth-service    | 3003 | api_only, in-memory       |
    When je fais une requête à users-service
    Then users-service doit répondre
    When users-service appelle articles-service
    Then la communication inter-services doit fonctionner

  # ==================== CONFIGURATION 10 : MODE TEST ====================

  Scenario: Configuration pour les tests automatisés
    Given un serveur Lithair avec configuration:
      | option              | valeur                    |
      | mode                | test                      |
      | persistence         | disabled                  |
      | port                | 0                         |
      | fixtures            | enabled                   |
      | fixtures_path       | tests/fixtures            |
      | reset_on_restart    | true                      |
    When je lance les tests automatisés
    Then le serveur doit démarrer en < 100ms
    And les fixtures doivent être chargées
    And après chaque test, la DB doit être réinitialisée

  # ==================== CONFIGURATION 11 : WEBSOCKETS + API ====================

  Scenario: API REST + WebSockets temps réel
    Given un serveur Lithair avec configuration:
      | option              | valeur                    |
      | mode                | full_stack                |
      | websockets          | enabled                   |
      | ws_port             | 3001                      |
    When un client se connecte en WebSocket
    Then la connexion doit être établie
    When je crée un article via l'API
    Then tous les clients WebSocket doivent recevoir la notification en temps réel

  # ==================== CONFIGURATION 12 : CONFIGURATION MINIMALE ====================

  Scenario: Configuration minimale (defaults)
    Given un serveur Lithair sans configuration explicite
    When je démarre le serveur
    Then il doit utiliser les valeurs par défaut:
      | option              | valeur par défaut         |
      | mode                | full_stack                |
      | persistence         | enabled                   |
      | port                | 8080                      |
      | frontend            | enabled                   |
    And le serveur doit fonctionner normalement

  # ==================== META-TEST : COMPILATION COMPLÈTE ====================

  Scenario: Compilation et démarrage du serveur final utilisateur
    Given le code source complet de Lithair
    When un utilisateur exécute "cargo build --release"
    Then la compilation doit réussir sans erreurs
    And un binaire "lithair" doit être créé
    When l'utilisateur exécute "./target/release/lithair --config prod.toml"
    Then le serveur doit démarrer avec succès
    And toutes les routes doivent être accessibles
    And la persistence doit fonctionner
    And les métriques doivent être disponibles
    And le serveur doit gérer 1000+ req/s
