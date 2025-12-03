# Test de Durabilité des Snapshots Lithair
# Vérifier que les snapshots accélèrent la récupération

Feature: Durabilité des Snapshots Lithair
  En tant que développeur
  Je veux vérifier que Lithair supporte les snapshots
  Pour accélérer le démarrage et la récupération après crash

  Background:
    Given la persistence multi-fichiers est activée

  # ==================== TEST CRÉATION SNAPSHOTS ====================

  @critical @snapshot
  Scenario: Création de snapshot pour un aggregate
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-basic"
    When je crée 100 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    And je crée un snapshot pour "articles" avec état '{"count": 100}'
    Then le snapshot pour "articles" doit exister
    And le snapshot pour "articles" doit contenir 100 événements
    And le snapshot pour "articles" doit avoir un CRC32 valide

  @critical @snapshot
  Scenario: Création de snapshot global
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-global"
    When je crée 50 événements sans aggregate_id
    And je flush tous les stores
    And je crée un snapshot global avec état '{"global_count": 50}'
    Then le snapshot global doit exister
    And le snapshot global doit contenir 50 événements

  # ==================== TEST RÉCUPÉRATION AVEC SNAPSHOT ====================

  @critical @snapshot @recovery
  Scenario: Récupération avec snapshot - moins d'événements à rejouer
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-recovery"
    When je crée 1000 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    And je crée un snapshot pour "articles" avec état '{"processed": 1000}'
    And je crée 100 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    Then le nombre total d'événements pour "articles" doit être 1100
    When je récupère les événements après snapshot pour "articles"
    Then je dois obtenir exactement 100 événements
    And tous ces événements doivent avoir un CRC32 valide

  @critical @snapshot @recovery
  Scenario: Récupération sans snapshot - tous les événements
    Given un store multi-fichiers dans "/tmp/lithair-no-snapshot"
    When je crée 500 "users" avec aggregate_id "users"
    And je flush tous les stores
    When je récupère les événements après snapshot pour "users"
    Then je dois obtenir exactement 500 événements

  # ==================== TEST INTÉGRITÉ SNAPSHOTS ====================

  @critical @snapshot @integrity
  Scenario: Détection de corruption de snapshot
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-corrupt"
    When je crée 100 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    And je crée un snapshot pour "articles" avec état '{"data": "test"}'
    And je corromps le fichier snapshot de "articles"
    Then le chargement du snapshot pour "articles" doit échouer avec erreur de corruption

  @snapshot @integrity
  Scenario: Snapshot avec état complexe
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-complex"
    When je crée 200 "products" avec aggregate_id "products"
    And je flush tous les stores
    And je crée un snapshot pour "products" avec état complexe
    Then le snapshot pour "products" doit exister
    And le chargement du snapshot pour "products" doit réussir
    And l'état récupéré doit être identique à l'état sauvegardé

  # ==================== TEST PERFORMANCE SNAPSHOTS ====================

  @performance @snapshot
  Scenario: Mesure du gain de performance avec snapshots
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-perf"
    When je crée 10000 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    And je mesure le temps de lecture de tous les événements "articles"
    And je crée un snapshot pour "articles" avec état '{"count": 10000}'
    And je crée 100 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    And je mesure le temps de lecture après snapshot pour "articles"
    Then le temps avec snapshot doit être au moins 10x plus rapide

  @performance @snapshot @threshold
  Scenario: Seuil automatique de création de snapshot
    Given un store multi-fichiers avec seuil de snapshot à 500 dans "/tmp/lithair-snapshot-threshold"
    When je crée 400 "logs" avec aggregate_id "logs"
    And je flush tous les stores
    Then un snapshot pour "logs" ne devrait pas être nécessaire
    When je crée 200 "logs" avec aggregate_id "logs"
    And je flush tous les stores
    Then un snapshot pour "logs" devrait être nécessaire

  # ==================== TEST MULTI-AGGREGATE SNAPSHOTS ====================

  @snapshot @multi
  Scenario: Snapshots indépendants par aggregate
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-multi"
    When je crée 100 "articles" avec aggregate_id "articles"
    And je crée 200 "users" avec aggregate_id "users"
    And je crée 150 "products" avec aggregate_id "products"
    And je flush tous les stores
    And je crée un snapshot pour "articles" avec état '{"articles": 100}'
    And je crée un snapshot pour "users" avec état '{"users": 200}'
    Then le snapshot pour "articles" doit contenir 100 événements
    And le snapshot pour "users" doit contenir 200 événements
    And le snapshot pour "products" ne doit pas exister
    And la liste des snapshots doit contenir 2 entrées

  @snapshot @multi @delete
  Scenario: Suppression de snapshot
    Given un store multi-fichiers dans "/tmp/lithair-snapshot-delete"
    When je crée 50 "temp" avec aggregate_id "temp"
    And je flush tous les stores
    And je crée un snapshot pour "temp" avec état '{"temp": true}'
    Then le snapshot pour "temp" doit exister
    When je supprime le snapshot pour "temp"
    Then le snapshot pour "temp" ne doit pas exister
