# Test de Durabilité Multi-Fichiers Lithair
# Vérifier que chaque structure a son propre fichier avec CRC32

Feature: Durabilité Multi-Fichiers Lithair
  En tant que développeur
  Je veux vérifier que Lithair supporte plusieurs structures
  Avec un fichier séparé par type de données et CRC32 validé

  Background:
    Given la persistence multi-fichiers est activée

  # ==================== TEST ISOLATION PAR STRUCTURE ====================

  @critical @multifile
  Scenario: Chaque structure a son propre fichier
    Given un store multi-fichiers dans "/tmp/lithair-multifile-test"
    When je crée 100 "articles" avec aggregate_id "articles"
    And je crée 50 "users" avec aggregate_id "users"
    And je crée 75 "products" avec aggregate_id "products"
    And je flush tous les stores
    Then le fichier "articles/events.raftlog" doit exister
    And le fichier "users/events.raftlog" doit exister
    And le fichier "products/events.raftlog" doit exister
    And le fichier "articles/events.raftlog" doit contenir exactement 100 lignes
    And le fichier "users/events.raftlog" doit contenir exactement 50 lignes
    And le fichier "products/events.raftlog" doit contenir exactement 75 lignes

  @critical @multifile @isolation
  Scenario: Isolation des données entre structures
    Given un store multi-fichiers dans "/tmp/lithair-isolation-test"
    When je crée 50 "articles" avec aggregate_id "articles"
    And je crée 30 "users" avec aggregate_id "users"
    And je flush tous les stores
    Then le fichier "articles/events.raftlog" ne doit contenir que des événements "articles"
    And le fichier "users/events.raftlog" ne doit contenir que des événements "users"
    And aucun événement "users" ne doit être dans "articles/events.raftlog"
    And aucun événement "articles" ne doit être dans "users/events.raftlog"

  # ==================== TEST CRC32 MULTI-FICHIERS ====================

  @critical @multifile @crc32
  Scenario: CRC32 validé sur tous les fichiers
    Given un store multi-fichiers dans "/tmp/lithair-multifile-crc32"
    When je crée 100 "articles" avec aggregate_id "articles"
    And je crée 100 "users" avec aggregate_id "users"
    And je flush tous les stores
    Then tous les événements dans "articles/events.raftlog" doivent avoir un CRC32 valide
    And tous les événements dans "users/events.raftlog" doivent avoir un CRC32 valide
    And le format de chaque ligne doit être "<crc32>:<json>"

  @critical @multifile @corruption
  Scenario: Détection de corruption par fichier
    Given un store multi-fichiers dans "/tmp/lithair-corruption-test"
    When je crée 50 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    And je corromps volontairement une ligne dans "articles/events.raftlog"
    Then la lecture de "articles/events.raftlog" doit détecter 1 événement corrompu
    And les autres fichiers ne doivent pas être affectés

  # ==================== TEST RECOVERY MULTI-FICHIERS ====================

  @critical @multifile @recovery
  Scenario: Recovery après crash avec multi-fichiers
    Given un store multi-fichiers dans "/tmp/lithair-multifile-crash"
    When je crée 200 "articles" avec aggregate_id "articles"
    And je crée 150 "users" avec aggregate_id "users"
    And je crée 100 "orders" avec aggregate_id "orders"
    And je flush tous les stores avec fsync
    And je simule un crash brutal
    When je recharge le store multi-fichiers depuis "/tmp/lithair-multifile-crash"
    Then je dois récupérer exactement 200 "articles"
    And je dois récupérer exactement 150 "users"
    And je dois récupérer exactement 100 "orders"
    And tous les CRC32 doivent être valides

  # ==================== TEST PERFORMANCE MULTI-FICHIERS ====================

  @performance @multifile
  Scenario: Performance d'écriture multi-fichiers
    Given un store multi-fichiers dans "/tmp/lithair-multifile-perf"
    When je mesure le temps pour créer 1000 événements répartis sur 5 structures
    And je flush tous les stores
    Then le temps total multifile doit être inférieur à 10 secondes
    And chaque structure doit avoir environ 200 événements
    And tous les fichiers doivent exister avec CRC32 valide

  @performance @multifile @concurrent
  Scenario: Écritures concurrentes sur plusieurs structures
    Given un store multi-fichiers dans "/tmp/lithair-concurrent-test"
    When je lance 5 tâches concurrentes écrivant chacune 100 événements sur une structure différente
    And j'attends la fin de toutes les tâches
    And je flush tous les stores
    Then chaque structure doit avoir exactement 100 événements
    And aucune donnée ne doit être mélangée entre structures
    And tous les CRC32 doivent être valides

  # ==================== TEST GLOBAL STORE ====================

  @multifile @global
  Scenario: Événements sans aggregate_id vont dans global
    Given un store multi-fichiers dans "/tmp/lithair-global-test"
    When je crée 50 événements sans aggregate_id
    And je crée 30 "articles" avec aggregate_id "articles"
    And je flush tous les stores
    Then le fichier "global/events.raftlog" doit contenir exactement 50 lignes
    And le fichier "articles/events.raftlog" doit contenir exactement 30 lignes

  # ==================== TEST LECTURE SÉLECTIVE ====================

  @multifile @read
  Scenario: Lecture sélective par structure
    Given un store multi-fichiers dans "/tmp/lithair-selective-read"
    When je crée 100 "articles" avec aggregate_id "articles"
    And je crée 100 "users" avec aggregate_id "users"
    And je crée 100 "products" avec aggregate_id "products"
    And je flush tous les stores
    When je lis uniquement la structure "articles"
    Then je dois obtenir exactement 100 événements
    And tous doivent être de type "articles"
    When je lis toutes les structures
    Then je dois obtenir exactement 300 événements au total
