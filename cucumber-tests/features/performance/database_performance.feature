# Performance et Intégrité de la Base de Données Lithair
# Tests critiques pour vérifier que sous charge, AUCUNE donnée n'est perdue

Feature: Performance et Intégrité de la Persistence Lithair
  En tant que développeur
  Je veux vérifier que Lithair sous charge
  Persiste TOUTES les données sans perte ni troncature

  Background:
    Given la persistence est activée par défaut

  # ==================== TESTS D'INTÉGRITÉ ====================

  Scenario: Créer 1000 articles et vérifier qu'ils sont TOUS persistés
    Given un serveur Lithair sur le port 20000 avec persistence "/tmp/lithair-integrity-1000"
    When je crée 1000 articles rapidement
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 1000 événements "ArticleCreated"
    And aucun événement ne doit être manquant
    And le checksum des événements doit être valide

  Scenario: CRUD complet - Créer, Modifier, Supprimer et vérifier la persistence
    Given un serveur Lithair sur le port 20001 avec persistence "/tmp/lithair-crud-test"
    When je crée 100 articles rapidement
    And je modifie 50 articles existants
    And je supprime 25 articles
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 100 événements "ArticleCreated"
    And le fichier events.raftlog doit contenir exactement 50 événements "ArticleUpdated"
    And le fichier events.raftlog doit contenir exactement 25 événements "ArticleDeleted"
    And l'état final doit avoir 75 articles actifs
    And tous les événements doivent être dans l'ordre chronologique

  Scenario: STRESS TEST - 100K articles avec CRUD complet et mesure de performance
    Given un serveur Lithair sur le port 20002 avec persistence "/tmp/lithair-stress-100k"
    When je crée 100000 articles rapidement
    And je modifie 10000 articles existants
    And je supprime 5000 articles
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 100000 événements "ArticleCreated"
    And le fichier events.raftlog doit contenir exactement 10000 événements "ArticleUpdated"
    And le fichier events.raftlog doit contenir exactement 5000 événements "ArticleDeleted"
    And l'état final doit avoir 95000 articles actifs
    And tous les événements doivent être dans l'ordre chronologique
    And j'arrête le serveur proprement

  Scenario: Créer 10000 articles et vérifier l'intégrité complète
    Given un serveur Lithair sur le port 20001 avec persistence "/tmp/lithair-integrity-10k"
    When je crée 10000 articles en parallèle avec 50 threads
    Then tous les 10000 articles doivent être persistés
    And le nombre d'événements dans events.raftlog doit être exactement 10000
    And aucun doublon ne doit exister
    And la séquence des IDs doit être continue de 0 à 9999

  Scenario: Test de charge avec vérification d'intégrité
    Given un serveur Lithair sur le port 20002 avec persistence "/tmp/lithair-load-test"
    When je lance 5000 requêtes POST concurrentes avec 100 threads
    And j'attends que toutes les écritures soient terminées
    Then le serveur doit avoir répondu à toutes les 5000 requêtes
    And le fichier events.raftlog doit contenir exactement 5000 événements
    And aucune erreur ne doit être présente dans les logs
    And le temps de réponse moyen doit être inférieur à 50ms

  # ==================== TESTS DE PERFORMANCE ====================

  Scenario: Performance d'écriture - 1000 req/s
    Given un serveur Lithair sur le port 20003 avec persistence "/tmp/lithair-perf-write"
    When je mesure la performance d'écriture sur 10 secondes
    Then le serveur doit traiter au moins 1000 requêtes par seconde
    And toutes les requêtes doivent être persistées
    And le taux d'erreur doit être de 0%
    And la latence p95 doit être inférieure à 100ms

  Scenario: Performance de lecture avec persistence
    Given un serveur Lithair sur le port 20004 avec persistence "/tmp/lithair-perf-read"
    And 5000 articles déjà créés et persistés
    When je mesure la performance de lecture sur 10 secondes
    Then le serveur doit traiter au moins 5000 requêtes par seconde
    And toutes les lectures doivent retourner des données valides
    And le taux d'erreur doit être de 0%
    And la latence p99 doit être inférieure à 20ms

  Scenario: Performance mixte lecture/écriture (80/20)
    Given un serveur Lithair sur le port 20005 avec persistence "/tmp/lithair-perf-mixed"
    When je lance un test mixte pendant 30 secondes avec:
      | Type     | Pourcentage | Concurrence |
      | Lecture  | 80%         | 100         |
      | Écriture | 20%         | 20          |
    Then le throughput total doit être supérieur à 2000 req/s
    And toutes les écritures doivent être persistées
    And le taux d'erreur doit être inférieur à 0.1%
    And la latence moyenne doit être inférieure à 30ms

  # ==================== TESTS DE PERSISTENCE SOUS CHARGE ====================

  Scenario: Persistence continue sous charge élevée
    Given un serveur Lithair sur le port 20006 avec persistence "/tmp/lithair-persist-load"
    When je lance une charge constante de 500 req/s pendant 60 secondes
    Then exactement 30000 événements doivent être persistés
    And le fichier events.raftlog doit avoir une taille cohérente
    And aucun événement ne doit être corrompu
    And la séquence temporelle doit être strictement croissante

  Scenario: Redémarrage avec données persistées
    Given un serveur Lithair sur le port 20007 avec persistence "/tmp/lithair-restart-test"
    And 1000 articles créés et persistés
    When j'arrête le serveur
    And je redémarre le serveur sur le même port avec la même persistence
    Then les 1000 articles doivent être présents en mémoire
    And je peux créer 1000 articles supplémentaires
    And le fichier events.raftlog doit contenir exactement 2000 événements
    And les IDs doivent être continus de 0 à 1999

  # ==================== TESTS D'INTÉGRITÉ AVANCÉS ====================

  Scenario: Vérification de l'ordre des événements
    Given un serveur Lithair sur le port 20008 avec persistence "/tmp/lithair-event-order"
    When je crée des événements dans cet ordre:
      | Type           | ID   |
      | ArticleCreated | art1 |
      | UserCreated    | usr1 |
      | ArticleUpdated | art1 |
      | CommentAdded   | cmt1 |
      | ArticleDeleted | art1 |
    Then les événements doivent être dans le fichier dans le même ordre
    And chaque événement doit avoir un timestamp strictement croissant
    And les relations entre événements doivent être préservées

  Scenario: Détection de corruption de données
    Given un serveur Lithair sur le port 20009 avec persistence "/tmp/lithair-corruption-test"
    When je crée 100 articles avec des checksums
    Then chaque article doit avoir un checksum valide dans la base
    And la somme totale des checksums doit correspondre
    And aucun article ne doit avoir de données corrompues
    And la vérification CRC32 doit passer

  # ==================== TESTS DE CHARGE EXTRÊME ====================

  Scenario: Charge extrême - 50000 articles
    Given un serveur Lithair sur le port 20010 avec persistence "/tmp/lithair-extreme-load"
    When je crée 50000 articles en 10 batches de 5000
    Then tous les 50000 articles doivent être persistés
    And le fichier events.raftlog doit faire au moins 5MB
    And aucun article ne doit manquer
    And la base doit rester cohérente
    And le serveur doit rester réactif (< 100ms de latence)

  Scenario: Test de concurrence extrême
    Given un serveur Lithair sur le port 20011 avec persistence "/tmp/lithair-concurrency"
    When je lance 1000 threads qui créent chacun 10 articles simultanément
    Then exactement 10000 articles doivent être persistés
    And aucun conflit de concurrence ne doit être détecté
    And tous les IDs doivent être uniques
    And aucun événement ne doit être dupliqué

  # ==================== TESTS DE TAILLE DE BASE ====================

  Scenario: Base de données volumineuse
    Given un serveur Lithair sur le port 20012 avec persistence "/tmp/lithair-large-db"
    When je crée des articles avec des contenus de 10KB chacun
    And je crée 1000 de ces articles
    Then le fichier events.raftlog doit faire au moins 10MB
    And tous les articles doivent être récupérables
    And le temps de lecture ne doit pas dégrader (< 50ms)
    And la base doit pouvoir être rechargée en moins de 5 secondes

  # ==================== TESTS DE SNAPSHOT ====================

  Scenario: Création de snapshot sous charge
    Given un serveur Lithair sur le port 20013 avec persistence "/tmp/lithair-snapshot"
    And la configuration snapshot est activée tous les 1000 événements
    When je crée 5000 articles
    Then au moins 5 snapshots doivent être créés
    And chaque snapshot doit être valide et récupérable
    And je peux restaurer depuis n'importe quel snapshot
    And les données après snapshot doivent être identiques

  # ==================== TESTS DE DURABILITÉ ====================

  Scenario: Durabilité fsync
    Given un serveur Lithair sur le port 20014 avec fsync activé
    When je crée 100 articles
    And je tue brutalement le serveur (SIGKILL)
    And je redémarre le serveur
    Then tous les 100 articles doivent être présents
    And aucune corruption ne doit être détectée

  Scenario: Durabilité sans fsync (mode performance)
    Given un serveur Lithair sur le port 20015 sans fsync
    When je crée 1000 articles rapidement
    Then le throughput doit être supérieur à 5000 req/s
    And au moins 95% des articles doivent être récupérables après crash
