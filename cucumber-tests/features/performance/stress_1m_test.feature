# Test de Stress 1 Million d'Articles - Lithair
# Vérification performance + cohérence + durabilité à grande échelle

Feature: STRESS TEST 1M - CRUD Mixte avec Vérification Intégrité
  En tant que développeur
  Je veux vérifier que Lithair peut gérer 1 million d'articles
  Avec des opérations CRUD mixtes et garantir l'intégrité des données

  Background:
    Given la persistence est activée par défaut

  # ==================== VALIDATION RAPIDE ====================

  Scenario: 10K articles - Validation rapide de l'architecture
    Given un serveur Lithair sur le port 20200 avec persistence "/tmp/lithair-stress-10k"

    # Phase 1 : Création
    When je crée 10000 articles rapidement
    Then je mesure le throughput de création

    # Phase 2 : Modifications
    When je modifie 2000 articles existants
    Then je mesure le throughput de modification

    # Phase 3 : Suppressions
    When je supprime 1000 articles
    Then je mesure le throughput de suppression

    # Phase 4 : Attente flush
    And j'attends 3 secondes pour le flush

    # Phase 5 : Vérifications
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 10000 événements "ArticleCreated"
    And le fichier events.raftlog doit contenir exactement 2000 événements "ArticleUpdated"
    And le fichier events.raftlog doit contenir exactement 1000 événements "ArticleDeleted"
    And l'état final doit avoir 9000 articles actifs
    And le nombre d'articles en mémoire doit égaler le nombre sur disque

    # Phase 6 : Métriques
    And j'arrête le serveur proprement
    And j'affiche les statistiques finales

  # ==================== STRESS TEST ULTIME ====================

  Scenario: 1 MILLION d'articles - CRUD mixte avec vérification complète
    Given un serveur Lithair sur le port 20200 avec persistence "/tmp/lithair-stress-1m"

    # Phase 1 : Création massive
    When je crée 1000000 articles rapidement
    Then je mesure le throughput de création

    # Phase 2 : Modifications sur un sous-ensemble
    When je modifie 200000 articles existants
    Then je mesure le throughput de modification

    # Phase 3 : Suppressions sur un sous-ensemble
    When je supprime 100000 articles
    Then je mesure le throughput de suppression

    # Phase 4 : Attente flush complet
    And j'attends 5 secondes pour le flush

    # Phase 5 : Vérifications d'intégrité
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 1000000 événements "ArticleCreated"
    And le fichier events.raftlog doit contenir exactement 200000 événements "ArticleUpdated"
    And le fichier events.raftlog doit contenir exactement 100000 événements "ArticleDeleted"
    And l'état final doit avoir 900000 articles actifs
    And tous les événements doivent être dans l'ordre chronologique
    And le nombre d'articles en mémoire doit égaler le nombre sur disque
    And tous les checksums doivent correspondre

    # Phase 6 : Métriques finales
    And j'arrête le serveur proprement
    And j'affiche les statistiques finales

  # ==================== TEST PERFORMANCE PURE ====================

  Scenario: 500K articles - Performance maximale en mode Performance
    Given un serveur Lithair sur le port 20201 avec persistence "/tmp/lithair-perf-500k"
    And le mode de durabilité est "Performance"

    When je crée 500000 articles rapidement
    Then le throughput doit être supérieur à 20000 articles/sec
    And le temps total doit être inférieur à 30 secondes

    When je supprime 100000 articles
    Then le throughput de suppression doit être supérieur à 15000 articles/sec

  # ==================== TEST COHÉRENCE SOUS CHARGE ====================

  Scenario: 100K articles - Cohérence garantie avec MaxDurability
    Given un serveur Lithair sur le port 20202 avec persistence "/tmp/lithair-coherence-100k"
    And le mode de durabilité est "MaxDurability"

    When je crée 100000 articles rapidement
    And je modifie 50000 articles existants
    And je supprime 25000 articles
    And j'attends 3 secondes pour le flush

    Then l'état final doit avoir 75000 articles actifs
    And le fichier events.raftlog doit contenir exactement 100000 événements "ArticleCreated"
    And le fichier events.raftlog doit contenir exactement 50000 événements "ArticleUpdated"
    And le fichier events.raftlog doit contenir exactement 25000 événements "ArticleDeleted"
    And le nombre d'articles en mémoire doit égaler le nombre sur disque
    And aucun événement ne doit être manquant

  # ==================== TEST RÉSILIENCE ====================

  Scenario: Résilience - 10K opérations mixtes aléatoires
    Given un serveur Lithair sur le port 20203 avec persistence "/tmp/lithair-resilience"

    When je lance 10000 opérations CRUD aléatoires
    And j'attends 2 secondes pour le flush

    Then tous les événements doivent être persistés
    And le nombre d'articles en mémoire doit égaler le nombre sur disque
    And la cohérence des données doit être validée
