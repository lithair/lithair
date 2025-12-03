@perf
Feature: TEST DIRECT MOTEUR - Performance Pure Lithair
  En tant que développeur
  Je veux tester le moteur Lithair DIRECTEMENT
  Sans overhead HTTP pour mesurer la vraie performance

  Background:
    Given le moteur Lithair est initialisé en mode MaxDurability

  # ==================== TEST 10K - VALIDATION RAPIDE ====================

  Scenario: 10K articles - Test direct du moteur
    Given un moteur avec persistence dans "/tmp/lithair-engine-10k"

    # Phase 1 : Création
    When je crée 10000 articles directement dans le moteur
    Then le throughput de création doit être supérieur à 200000 articles/sec

    # Phase 2 : Modifications
    When je modifie 2000 articles directement dans le moteur
    Then le throughput de modification doit être supérieur à 200000 articles/sec

    # Phase 3 : Suppressions
    When je supprime 1000 articles directement dans le moteur
    Then le throughput de suppression doit être supérieur à 200000 articles/sec

    # Phase 4 : Flush et vérifications
    And j'attends le flush complet du moteur
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 13000 événements
    And le moteur doit avoir 9000 articles en mémoire
    And tous les événements doivent être persistés

  # ==================== TEST 100K - MONTÉE EN CHARGE ====================

  Scenario: 100K articles - Test direct montée en charge
    Given un moteur avec persistence dans "/tmp/lithair-engine-100k"

    When je crée 100000 articles directement dans le moteur
    Then le throughput de création doit être supérieur à 200000 articles/sec

    When je modifie 20000 articles directement dans le moteur
    Then le throughput de modification doit être supérieur à 200000 articles/sec

    When je supprime 10000 articles directement dans le moteur
    And j'attends le flush complet du moteur

    Then le fichier events.raftlog doit contenir exactement 130000 événements
    And le moteur doit avoir 90000 articles en mémoire

  # ==================== TEST 1M - STRESS ULTIME ====================

  @stress
  Scenario: 1 MILLION d'articles - Test direct stress ultime
    Given un moteur avec persistence dans "/tmp/lithair-engine-1m"

    # Phase 1 : Création massive
    When je crée 1000000 articles directement dans le moteur
    Then le throughput de création doit être supérieur à 300000 articles/sec
    And le temps de création doit être inférieur à 5 secondes

    # Phase 2 : Modifications
    When je modifie 200000 articles directement dans le moteur
    Then le throughput de modification doit être supérieur à 200000 articles/sec

    # Phase 3 : Suppressions
    When je supprime 100000 articles directement dans le moteur
    Then le throughput de suppression doit être supérieur à 200000 articles/sec

    # Phase 4 : Vérifications complètes
    And j'attends le flush complet du moteur
    Then le fichier events.raftlog doit exister
    And le fichier events.raftlog doit contenir exactement 1300000 événements
    And le moteur doit avoir 900000 articles en mémoire
    And la taille du fichier events.raftlog doit être environ 85 MB
    And tous les événements doivent être dans l'ordre chronologique
    And aucun événement ne doit être manquant

  # ==================== TEST COHÉRENCE ====================

  Scenario: Vérification cohérence mémoire/disque avec 50K articles
    Given un moteur avec persistence dans "/tmp/lithair-engine-coherence"

    When je crée 50000 articles directement dans le moteur
    And je modifie 10000 articles directement dans le moteur
    And je supprime 5000 articles directement dans le moteur
    And j'attends le flush complet du moteur

    Then le nombre d'articles en mémoire doit égaler le nombre reconstruit depuis le disque
    And tous les checksums doivent correspondre entre mémoire et disque
