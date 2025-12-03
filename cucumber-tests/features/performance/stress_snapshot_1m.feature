# Stress Test 1 Million d'événements avec Snapshots
# Validation de la performance et fiabilité du système de snapshots à grande échelle

Feature: STRESS TEST - Snapshots avec volumes massifs
  En tant que développeur
  Je veux vérifier que les snapshots fonctionnent correctement à grande échelle
  Pour garantir des temps de récupération acceptables même avec des millions d'événements

  Background:
    Given la persistence multi-fichiers est activée

  # ==================== VALIDATION RAPIDE 10K ====================

  @quick
  Scenario: 10K événements - Validation rapide des snapshots
    Given un store multi-fichiers avec seuil de snapshot à 1000 dans "/tmp/lithair-stress-snap-10k"

    # Phase 1 : Création massive
    When je crée 10000 "Article" avec aggregate_id "articles"
    And je flush tous les stores

    # Phase 2 : Création snapshot
    When je crée un snapshot pour "articles" avec état complexe de 10000 éléments
    Then le snapshot pour "articles" doit exister
    And le snapshot pour "articles" doit avoir un CRC32 valide
    And le snapshot pour "articles" doit contenir 10000 événements

    # Phase 3 : Ajout événements supplémentaires
    When je crée 100 "Article" avec aggregate_id "articles"
    And je flush tous les stores

    # Phase 4 : Validation events à rejouer
    When je récupère les événements après snapshot pour "articles"
    Then le nombre d'événements à rejouer doit être 100

    # Phase 5 : Validation intégrité
    And tous les CRC32 doivent être valides
    And le nombre total d'événements pour "articles" doit être 10100

  # ==================== TEST 100K ÉVÉNEMENTS ====================

  @medium
  Scenario: 100K événements - Performance snapshots avec charge modérée
    Given un store multi-fichiers avec seuil de snapshot à 10000 dans "/tmp/lithair-stress-snap-100k"

    # Phase 1 : Création 100K événements
    When je crée 100000 événements avec throughput mesuré pour "orders"
    Then le throughput de création doit être supérieur à 100 evt/s

    # Phase 2 : Snapshot après 100K
    When je crée un snapshot pour "orders" avec état de 100000 éléments
    And je flush tous les stores
    Then le snapshot pour "orders" doit exister
    And le snapshot pour "orders" doit contenir 100000 événements

    # Phase 3 : Ajout 1000 événements post-snapshot
    When je crée 1000 "Order" avec aggregate_id "orders"
    And je flush tous les stores

    # Phase 4 : Validation récupération après crash
    When je simule un crash brutal
    And je recharge le store multi-fichiers depuis "/tmp/lithair-stress-snap-100k"

    # Phase 5 : Validation des événements à rejouer
    When je récupère les événements après snapshot pour "orders"
    Then le nombre d'événements à rejouer doit être 1000
    And le nombre total d'événements pour "orders" doit être 101000
    And tous les CRC32 doivent être valides

  # ==================== TEST 500K ÉVÉNEMENTS ====================

  @large
  Scenario: 500K événements - Stress test haute performance
    Given un store multi-fichiers avec seuil de snapshot à 50000 dans "/tmp/lithair-stress-snap-500k"

    # Phase 1 : Création par batch de 100K
    When je crée 500000 événements par batch de 100000 pour "products"
    Then le temps total de création doit être inférieur à 60 secondes

    # Phase 2 : Création snapshot
    When je crée un snapshot pour "products" avec état de 500000 éléments
    And je flush tous les stores avec fsync

    # Phase 3 : Validation snapshot
    Then le snapshot pour "products" doit exister
    And le snapshot pour "products" doit avoir un CRC32 valide

    # Phase 4 : Événements post-snapshot
    When je crée 5000 "Product" avec aggregate_id "products"
    And je flush tous les stores

    # Phase 5 : Recovery test
    When je simule un crash brutal
    And je recharge le store multi-fichiers depuis "/tmp/lithair-stress-snap-500k"

    # Phase 6 : Performance
    When je mesure le temps de récupération complète pour "products"
    And je mesure le temps de récupération après snapshot pour "products"
    Then la récupération avec snapshot doit être au moins 80x plus rapide

  # ==================== STRESS TEST 1 MILLION ====================

  @stress @1m
  Scenario: 1 MILLION d'événements - Validation ultime des snapshots
    Given un store multi-fichiers avec seuil de snapshot à 100000 dans "/tmp/lithair-stress-snap-1m"

    # Phase 1 : Création 1M événements
    When je crée 1000000 événements par batch de 100000 pour "transactions"
    Then le temps total de création doit être inférieur à 120 secondes
    And le throughput moyen doit être supérieur à 8000 evt/s

    # Phase 2 : Snapshot après 1M
    When je crée un snapshot pour "transactions" avec état de 1000000 éléments
    And je flush tous les stores avec fsync

    # Phase 3 : Validation snapshot
    Then le snapshot pour "transactions" doit exister
    And le snapshot pour "transactions" doit contenir 1000000 événements
    And le snapshot pour "transactions" doit avoir un CRC32 valide
    And la taille du fichier snapshot doit être raisonnable

    # Phase 4 : Événements post-snapshot
    When je crée 10000 "Transaction" avec aggregate_id "transactions"
    And je flush tous les stores

    # Phase 5 : Test récupération après crash
    When je simule un crash brutal
    And je recharge le store multi-fichiers depuis "/tmp/lithair-stress-snap-1m"

    # Phase 6 : Performance recovery
    When je mesure le temps de récupération complète pour "transactions"
    And je mesure le temps de récupération après snapshot pour "transactions"
    Then la récupération avec snapshot doit être au moins 90x plus rapide
    And le temps de récupération avec snapshot doit être inférieur à 5 secondes

    # Phase 7 : Validation finale
    And le nombre total d'événements pour "transactions" doit être 1010000
    And tous les CRC32 doivent être valides

  # ==================== MULTI-AGGREGATE STRESS ====================

  @multi
  Scenario: Multi-aggregate - 100K événements répartis sur 100 aggregates
    Given un store multi-fichiers avec seuil de snapshot à 500 dans "/tmp/lithair-stress-multi"

    # Phase 1 : Création distribuée
    When je crée 100000 événements répartis sur 100 aggregates
    And je flush tous les stores

    # Phase 2 : Création snapshots pour chaque aggregate
    When je crée des snapshots pour tous les aggregates

    # Phase 3 : Validation
    Then 100 snapshots doivent exister
    And tous les snapshots doivent avoir un CRC32 valide

    # Phase 4 : Ajout post-snapshot
    When je crée 1000 événements répartis sur 100 aggregates
    And je flush tous les stores

    # Phase 5 : Recovery test
    When je simule un crash brutal
    And je recharge le store multi-fichiers depuis "/tmp/lithair-stress-multi"

    # Phase 6 : Validation finale
    Then chaque aggregate doit avoir 1010 événements
    And la récupération distribuée doit utiliser les snapshots

  # ==================== SNAPSHOT ROTATION ====================

  @rotation
  Scenario: Rotation de snapshots - Gestion de multiples snapshots
    Given un store multi-fichiers avec seuil de snapshot à 1000 dans "/tmp/lithair-stress-rotation"

    # Phase 1 : Premier lot + snapshot
    When je crée 5000 "Event" avec aggregate_id "rotating"
    And je flush tous les stores
    When je crée un snapshot pour "rotating" avec état de 5000 éléments
    Then le snapshot pour "rotating" doit exister

    # Phase 2 : Deuxième lot + nouveau snapshot
    When je crée 5000 "Event" avec aggregate_id "rotating"
    And je flush tous les stores
    When je crée un snapshot pour "rotating" avec état de 10000 éléments
    Then le snapshot pour "rotating" doit contenir 10000 événements

    # Phase 3 : Troisième lot + snapshot final
    When je crée 5000 "Event" avec aggregate_id "rotating"
    And je flush tous les stores
    When je crée un snapshot pour "rotating" avec état de 15000 éléments
    Then le snapshot pour "rotating" doit contenir 15000 événements

    # Phase 4 : Validation récupération
    When je simule un crash brutal
    And je recharge le store multi-fichiers depuis "/tmp/lithair-stress-rotation"
    Then le nombre total d'événements pour "rotating" doit être 15000
